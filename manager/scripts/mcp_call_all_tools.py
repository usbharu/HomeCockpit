#!/usr/bin/env python3
"""Call every Manager MCP tool against a live server and record outcomes."""

from __future__ import annotations

import json
import socket
import sys
from typing import Any

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 28765


def post(body: dict[str, Any]) -> str:
    payload = json.dumps(body, separators=(",", ":"))
    host = f"127.0.0.1:{PORT}"
    origin = f"http://{host}"
    request = (
        f"POST /mcp HTTP/1.1\r\n"
        f"Host: {host}\r\n"
        f"Origin: {origin}\r\n"
        f"Content-Type: application/json\r\n"
        f"Accept: application/json, text/event-stream\r\n"
        f"MCP-Protocol-Version: 2025-11-25\r\n"
        f"Connection: close\r\n"
        f"Content-Length: {len(payload)}\r\n\r\n"
        f"{payload}"
    )
    with socket.create_connection(("127.0.0.1", PORT), timeout=120) as sock:
        sock.sendall(request.encode())
        chunks: list[bytes] = []
        while True:
            part = sock.recv(1_048_576)
            if not part:
                break
            chunks.append(part)
        return b"".join(chunks).decode("utf-8", "replace")


def parse_bodies(http: str) -> list[Any]:
    _, _, rest = http.partition("\r\n\r\n")
    out: list[Any] = []
    for line in rest.splitlines():
        line = line.strip()
        if line.startswith("data:"):
            line = line[5:].strip()
        if not line or line.startswith(":"):
            continue
        try:
            out.append(json.loads(line))
        except json.JSONDecodeError:
            pass
    if not out and "{" in rest:
        for line in rest.splitlines():
            line = line.strip()
            if line.startswith("data:"):
                line = line[5:].strip()
            if line.startswith("{") and line.endswith("}"):
                try:
                    out.append(json.loads(line))
                except json.JSONDecodeError:
                    pass
    return out


def call_tool(name: str, arguments: dict[str, Any] | None = None) -> dict[str, Any]:
    post(
        {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": {"name": "mcp-call-all", "version": "1"},
            },
        }
    )
    rid = abs(hash(name)) % 1_000_000
    http = post(
        {
            "jsonrpc": "2.0",
            "id": rid,
            "method": "tools/call",
            "params": {"name": name, "arguments": arguments or {}},
        }
    )
    bodies = parse_bodies(http)
    for body in bodies:
        if isinstance(body, dict) and body.get("id") == rid:
            return body
    return {"raw": http[-500:]}


def is_error(result: dict[str, Any]) -> str | None:
    if result.get("error"):
        return str(result["error"])
    if "result" in result and result["result"].get("isError"):
        content = result["result"].get("content") or []
        if content and isinstance(content[0], dict):
            return content[0].get("text", "tool error")
    return None


def structured(result: dict[str, Any]) -> Any:
    if "result" not in result:
        return None
    r = result["result"]
    if "structuredContent" in r:
        return r["structuredContent"]
    return r.get("content")


ADAPTER_SOURCE = r'''
docdata["TEST_MODULE"] = {
  "Panel": {
    "MODE_SW": {
      "category": "Panel",
      "control_type": "selector",
      "description": "Mode switch",
      "identifier": "MODE_SW",
      "inputs": [
        {"interface": "fixed_step", "description": "step"},
        {"interface": "set_state", "description": "set", "max_value": 2}
      ],
      "outputs": [{
        "address": 4096,
        "mask": 3,
        "shift_by": 0,
        "max_value": 2,
        "type": "integer"
      }]
    }
  }
};
'''

HCP_SET_PACKET = {
    "ControlEvent": {
        "seq": 0,
        "control_id": 65280,
        "event": "RequestDeviceHello",
    }
}


def main() -> int:
    post(
        {
            "jsonrpc": "2.0",
            "id": 0,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": {"name": "mcp-call-all", "version": "1"},
            },
        }
    )
    snap = call_tool("manager_snapshot")
    snap_err = is_error(snap)
    snap_data = structured(snap) if not snap_err else {}
    default_dcs = (snap_data or {}).get("dcsbiosConfig") or {
        "exportHost": "127.0.0.1",
        "exportPort": 5010,
        "commandHost": "127.0.0.1",
        "commandPort": 7778,
        "commandTransport": "udp",
    }

    imcp_hex = structured(call_tool("imcp_encode", {"to": 255, "from": 1, "kind": "ping"}))
    imcp_wire = (imcp_hex or {}).get("hex", "")

    hcp_hex = structured(
        call_tool("hcp_encode", {"kind": "set", "packet": HCP_SET_PACKET})
    )
    hcp_wire = (hcp_hex or {}).get("hex", "")

    adapter_request = {
        "adapterId": "dcs-bios",
        "profileId": "TEST_MODULE",
        "label": "Test",
        "aircraftNames": ["TEST"],
        "source": ADAPTER_SOURCE,
    }

    calls: list[tuple[str, dict[str, Any] | None]] = [
        ("manager_snapshot", {}),
        ("manager_logs", {}),
        ("manager_devices", {}),
        ("manager_scan_serial_ports", {}),
        ("manager_preview_adapter_profile", {"request": adapter_request}),
        ("manager_start_learn", {
            "request": {
                "roleId": "left-ddi",
                "logicalControlId": "button-0",
                "mode": "append",
            }
        }),
        ("manager_cancel_learn", {}),
        ("manager_refresh_devices", {}),
        ("manager_trigger_role_input", {
            "roleId": "left-ddi",
            "logicalControlId": "button-0",
            "eventKind": "button-pushed",
        }),
        ("manager_save_dcsbios_config", {"config": default_dcs}),
        ("manager_save_endpoints", {"deviceEndpoints": []}),
        ("manager_save_role_assignments", {"deviceRoleAssignments": []}),
        ("manager_save_adapter_mappings", {"adapterMappings": []}),
        ("manager_save_adapter_profile", {"request": adapter_request}),
        ("dcsbios_status", {}),
        ("dcsbios_memory_read", {"address": 0, "length": 4}),
        ("dcsbios_recent_packets", {"sinceId": 0, "limit": 10}),
        ("dcsbios_packet_read", {"id": 1}),
        ("dcsbios_start", {}),
        ("dcsbios_stop", {}),
        ("dcsbios_send_command", {"rawCommand": "PING 1\n"}),
        ("hcp_encode", {"kind": "set", "packet": HCP_SET_PACKET}),
        ("hcp_decode", {"hex": hcp_wire}),
        ("hcp_send", {"deviceId": "missing", "kind": "set", "packet": HCP_SET_PACKET}),
        ("imcp_encode", {"to": 255, "from": 1, "kind": "ping"}),
        ("imcp_decode", {"hex": imcp_wire}),
        ("imcp_send", {
            "endpointId": "missing",
            "frame": {"to": 255, "from": 1, "kind": "ping"},
        }),
        ("imcp_recent_frames", {"sinceId": 0, "limit": 10}),
        ("hcp_recent_packets", {"sinceId": 0, "limit": 10}),
    ]

    report: list[dict[str, Any]] = []
    ok = 0
    for name, args in calls:
        result = call_tool(name, args)
        err = is_error(result)
        body = structured(result)
        entry = {
            "tool": name,
            "ok": err is None,
            "error": err,
            "result_type": type(body).__name__,
            "result_keys": sorted(body.keys()) if isinstance(body, dict) else None,
            "result_preview": (
                json.dumps(body)[:200] if body is not None and not isinstance(body, list) else
                (f"array[{len(body)}]" if isinstance(body, list) else str(body)[:200])
            ),
        }
        report.append(entry)
        if err is None:
            ok += 1
        print(json.dumps(entry, ensure_ascii=False))

    print(
        json.dumps(
            {"port": PORT, "called": len(calls), "ok": ok, "failed": len(calls) - ok},
            indent=2,
        ),
        file=sys.stderr,
    )
    expected_fail = {
        "manager_preview_adapter_profile",
        "manager_save_adapter_profile",
        "manager_trigger_role_input",
        "dcsbios_memory_read",
        "dcsbios_packet_read",
        "hcp_send",
        "imcp_send",
    }
    unexpected = [
        e for e in report if not e["ok"] and e["tool"] not in expected_fail
    ]
    if unexpected:
        print("Unexpected failures:", unexpected, file=sys.stderr)
        return 1
    print(
        f"All {len(calls)} tools called; {ok} succeeded; "
        f"{len(expected_fail)} expected failures in empty environment.",
        file=sys.stderr,
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
