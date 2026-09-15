#!/usr/bin/env python3
"""Read-only DCS-BIOS UDP export-stream detector.

The DCS-BIOS export is a logical byte stream carried in UDP chunks. The
detector therefore keeps parser state across datagrams instead of requiring
every datagram to start with the frame-sync marker.
"""

from __future__ import annotations

import argparse
import ipaddress
import socket
import sys
import time
from dataclasses import dataclass, field
from typing import List, Optional, Sequence, Tuple


DEFAULT_HOST = "239.255.50.10"
DEFAULT_PORT = 5010
SYNC = b"\x55\x55\x55\x55"
MAX_SYNC_PREFIX = len(SYNC) - 1


@dataclass(frozen=True)
class WriteRecord:
    address: int
    payload: bytes


def parse_export_datagram(data: bytes) -> Tuple[bool, List[WriteRecord]]:
    """Strictly validate a frame known to fit in one datagram.

    This strict helper is useful for unit checks. The live detector below uses
    ExportStreamParser because DCS-BIOS can split a logical frame across UDP
    datagrams.
    """

    if not data.startswith(SYNC):
        return False, []

    records: List[WriteRecord] = []
    offset = len(SYNC)
    while offset < len(data):
        if len(data) - offset < 4:
            return False, records

        address = int.from_bytes(data[offset : offset + 2], "little")
        length = int.from_bytes(data[offset + 2 : offset + 4], "little")
        payload_start = offset + 4
        payload_end = payload_start + length

        if address % 2 != 0 or length % 2 != 0 or payload_end > len(data):
            return False, records

        records.append(WriteRecord(address=address, payload=data[payload_start:payload_end]))
        offset = payload_end

    return bool(records), records


@dataclass
class ExportStreamParser:
    """Incrementally validate DCS-BIOS frames across UDP datagrams."""

    buffer: bytearray = field(default_factory=bytearray)
    in_frame: bool = False
    frame_records: int = 0
    sync_count: int = 0
    records_seen: int = 0
    complete_frames: int = 0
    open_frames: int = 0
    malformed_segments: int = 0

    def feed(self, data: bytes) -> None:
        self.buffer.extend(data)
        self._drain()

    def finish_capture(self) -> None:
        if self.in_frame and self.frame_records:
            self.open_frames += 1
        self.in_frame = False

    @property
    def observed_frames(self) -> int:
        return self.complete_frames + self.open_frames

    def _drain(self) -> None:
        while True:
            if not self.in_frame:
                sync_offset = self.buffer.find(SYNC)
                if sync_offset < 0:
                    self._keep_possible_sync_prefix()
                    return
                del self.buffer[: sync_offset + len(SYNC)]
                self.in_frame = True
                self.frame_records = 0
                self.sync_count += 1

            if self.buffer.startswith(SYNC):
                self._finish_frame()
                continue

            if len(self.buffer) < 4:
                return

            address = int.from_bytes(self.buffer[0:2], "little")
            length = int.from_bytes(self.buffer[2:4], "little")
            payload_end = 4 + length
            next_sync = self.buffer.find(SYNC, 1)

            if address % 2 != 0 or length % 2 != 0 or (next_sync >= 0 and next_sync < payload_end):
                self._mark_malformed_and_resync(next_sync)
                continue

            if len(self.buffer) < payload_end:
                if next_sync >= 0:
                    self._mark_malformed_and_resync(next_sync)
                    continue
                return

            del self.buffer[:payload_end]
            self.frame_records += 1
            self.records_seen += 1
            _ = address

    def _finish_frame(self) -> None:
        if self.frame_records:
            self.complete_frames += 1
        self.in_frame = False
        self.frame_records = 0

    def _mark_malformed_and_resync(self, sync_offset: int) -> None:
        self.malformed_segments += 1
        self.in_frame = False
        self.frame_records = 0
        if sync_offset >= 0:
            del self.buffer[:sync_offset]
        else:
            self._keep_possible_sync_prefix()

    def _keep_possible_sync_prefix(self) -> None:
        if len(self.buffer) > MAX_SYNC_PREFIX:
            del self.buffer[:-MAX_SYNC_PREFIX]


def ascii_preview(data: bytes, limit: int = 96) -> str:
    return "".join(chr(byte) if 32 <= byte < 127 else "." for byte in data[:limit])


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Listen for and validate DCS-BIOS UDP export data."
    )
    parser.add_argument(
        "--host",
        default=DEFAULT_HOST,
        help=(
            "Multicast group to join, or an IPv4 address label for a unicast listener "
            f"(default: {DEFAULT_HOST})"
        ),
    )
    parser.add_argument(
        "--port", type=int, default=DEFAULT_PORT, help=f"UDP port (default: {DEFAULT_PORT})"
    )
    parser.add_argument(
        "--interface",
        default="0.0.0.0",
        help="IPv4 interface address for multicast membership (default: 0.0.0.0)",
    )
    parser.add_argument(
        "--seconds", type=float, default=8.0, help="listen duration (default: 8 seconds)"
    )
    parser.add_argument("--count", type=int, default=0, help="stop after N datagrams; 0 means use --seconds")
    parser.add_argument("--show-hex", action="store_true", help="print the first datagram as hex")
    return parser


def validate_args(args: argparse.Namespace) -> Tuple[ipaddress.IPv4Address, ipaddress.IPv4Address]:
    try:
        host = ipaddress.ip_address(args.host)
        interface = ipaddress.ip_address(args.interface)
    except ValueError as error:
        raise ValueError(f"host and interface must be IPv4 addresses: {error}") from error

    if not isinstance(host, ipaddress.IPv4Address):
        raise ValueError("--host must be an IPv4 address")
    if not isinstance(interface, ipaddress.IPv4Address):
        raise ValueError("--interface must be an IPv4 address")
    if not 1 <= args.port <= 65535:
        raise ValueError("--port must be between 1 and 65535")
    if args.seconds <= 0:
        raise ValueError("--seconds must be greater than zero")
    if args.count < 0:
        raise ValueError("--count cannot be negative")

    return host, interface


def listen(args: argparse.Namespace) -> int:
    host, interface = validate_args(args)
    is_multicast = host.is_multicast
    # macOS may reject a second wildcard bind even with SO_REUSEADDR. Binding
    # the group address permits observation while Manager owns the wildcard.
    bind_address = (
        (str(host), args.port)
        if sys.platform == "darwin" and is_multicast
        else ("", args.port)
    )
    stream = ExportStreamParser()
    sources = set()
    received = 0
    sync_datagrams = 0
    continuation_datagrams = 0
    unsynchronized_datagrams = 0
    first_packet_hex: Optional[str] = None

    mode = f"multicast on {interface}" if is_multicast else "unicast listener"
    print(f"Listening for DCS-BIOS export data on {host}:{args.port} ({mode}).")

    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM, socket.IPPROTO_UDP)
    try:
        sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        sock.bind(bind_address)

        if is_multicast:
            membership = socket.inet_aton(str(host)) + socket.inet_aton(str(interface))
            sock.setsockopt(socket.IPPROTO_IP, socket.IP_ADD_MEMBERSHIP, membership)

        deadline = time.monotonic() + args.seconds
        while time.monotonic() < deadline:
            remaining = max(0.05, deadline - time.monotonic())
            sock.settimeout(min(0.5, remaining))
            try:
                data, source = sock.recvfrom(65535)
            except socket.timeout:
                continue

            received += 1
            sources.add(f"{source[0]}:{source[1]}")
            if first_packet_hex is None:
                first_packet_hex = data.hex(" ")

            starts_sync = data.startswith(SYNC)
            was_in_frame = stream.in_frame
            if starts_sync:
                sync_datagrams += 1
                kind = "frame-start"
            elif was_in_frame:
                continuation_datagrams += 1
                kind = "continuation"
            else:
                unsynchronized_datagrams += 1
                kind = "unsynchronized"

            stream.feed(data)
            if received <= 3:
                print(
                    f"packet {received}: {kind}, {len(data)} bytes, "
                    f"source {source[0]}:{source[1]}, "
                    f"logical records seen {stream.records_seen}"
                )
                print(f"  preview: {ascii_preview(data)}")

            if args.count and received >= args.count:
                break
    except OSError as error:
        print(f"Socket error on {host}:{args.port}: {error}", file=sys.stderr)
        return 3
    finally:
        sock.close()

    stream.finish_capture()
    source_summary = ", ".join(sorted(sources)) if sources else "none"
    print(
        f"Summary: {received} datagram(s), {stream.observed_frames} logical frame(s) observed "
        f"({stream.complete_frames} closed, {stream.open_frames} open at capture end), "
        f"{stream.records_seen} write record(s), {stream.malformed_segments} malformed segment(s)."
    )
    print(
        f"Chunks: {sync_datagrams} frame-start, {continuation_datagrams} continuation, "
        f"{unsynchronized_datagrams} unsynchronized."
    )
    print(f"Sources: {source_summary}")
    if args.show_hex and first_packet_hex is not None:
        print(f"First datagram hex: {first_packet_hex}")

    if stream.sync_count and stream.records_seen:
        return 0
    if received:
        return 1
    return 2


def main(argv: Optional[Sequence[str]] = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    try:
        return listen(args)
    except ValueError as error:
        parser.error(str(error))
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
