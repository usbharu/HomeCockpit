#!/usr/bin/env python3
"""Minimal Streamable HTTP client for Manager MCP (initialize + one JSON-RPC call)."""

from __future__ import annotations

import argparse
import json
import socket
import sys
from typing import Any


def post_mcp(port: int, body: dict[str, Any]) -> str:
    payload = json.dumps(body, separators=(",", ":"))
    host = f"127.0.0.1:{port}"
    origin = f"http://{host}"
    request = (
        f"POST /mcp HTTP/1.1\r\n"
        f"Host: {host}\r\n"
        f"Origin: {origin}\r\n"
        f"Content-Type: application/json\r\n"
        f"Accept: application/json, text/event-stream\r\n"
        f"MCP-Protocol-Version: 2025-11-25\r\n"
        f"Connection: close\r\n"
        f"Content-Length: {len(payload)}\r\n"
        f"\r\n"
        f"{payload}"
    )
    with socket.create_connection(("127.0.0.1", port), timeout=5) as sock:
        sock.sendall(request.encode())
        return sock.recv(1_000_000).decode("utf-8", "replace")


def extract_json_bodies(http_response: str) -> list[Any]:
    bodies: list[Any] = []
    _, _, rest = http_response.partition("\r\n\r\n")
    for line in rest.splitlines():
        line = line.strip()
        if line.startswith("data:"):
            line = line[5:].strip()
        if not line or line.startswith(":"):
            continue
        try:
            bodies.append(json.loads(line))
        except json.JSONDecodeError:
            if line.startswith("{") or line.startswith("["):
                try:
                    bodies.append(json.loads(line))
                except json.JSONDecodeError:
                    pass
    if not bodies and "{" in rest:
        start = rest.find("{")
        end = rest.rfind("}") + 1
        if end > start:
            bodies.append(json.loads(rest[start:end]))
    return bodies


def mcp_session(port: int) -> list[Any]:
    init = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": {"name": "mcp-tool-eval", "version": "1"},
        },
    }
    post_mcp(port, init)
    listed = post_mcp(
        port,
        {"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}},
    )
    return extract_json_bodies(listed)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, default=28765)
    parser.add_argument("--list-tools", action="store_true")
    args = parser.parse_args()
    bodies = mcp_session(args.port)
    if args.list_tools:
        for body in bodies:
            if isinstance(body, dict) and "result" in body:
                tools = body.get("result", {}).get("tools")
                if tools:
                    print(json.dumps({"tools": tools}, indent=2))
                    return 0
        print(json.dumps(bodies, indent=2), file=sys.stderr)
        return 1
    print(json.dumps(bodies, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
