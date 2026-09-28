#!/usr/bin/env python3
"""Emit JSON bundle of live MCP instructions, tools/list, and call-all report."""

from __future__ import annotations

import json
import socket
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent
PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 28765


def post(body: dict) -> str:
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
    with socket.create_connection(("127.0.0.1", PORT), timeout=30) as sock:
        sock.sendall(request.encode())
        chunks: list[bytes] = []
        while True:
            part = sock.recv(1_048_576)
            if not part:
                break
            chunks.append(part)
        return b"".join(chunks).decode("utf-8", "replace")


def parse_tools(list_http: str) -> list[dict]:
    for line in list_http.splitlines():
        line = line.strip()
        if line.startswith("data:"):
            line = line[5:].strip()
        if not line.startswith("{"):
            continue
        try:
            body = json.loads(line)
        except json.JSONDecodeError:
            continue
        tools = body.get("result", {}).get("tools")
        if tools:
            return tools
    return []


def extract_instructions(init_http: str) -> str:
    marker = "HomeCockpit Manager MCP"
    start = init_http.find(marker)
    if start < 0:
        return ""
    return init_http[start : start + 1400].split('"')[0]


def main() -> int:
    init = post(
        {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": {"name": "eval-bundle", "version": "1"},
            },
        }
    )
    tools = parse_tools(
        post({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}})
    )
    proc = subprocess.run(
        [sys.executable, str(ROOT / "mcp_call_all_tools.py"), str(PORT)],
        capture_output=True,
        text=True,
        check=False,
    )
    report = [
        json.loads(line)
        for line in proc.stdout.splitlines()
        if line.strip().startswith("{") and '"tool"' in line
    ]
    scenarios = json.loads((ROOT / "mcp_tool_scenarios.json").read_text())
    bundle = {
        "server_instructions": extract_instructions(init),
        "tools": [
            {
                "name": t["name"],
                "description": t.get("description"),
                "inputSchema": t.get("inputSchema"),
            }
            for t in tools
        ],
        "live_call_report": report,
        "call_script_stderr": proc.stderr.strip(),
        "selection_scenarios": scenarios,
    }
    json.dump(bundle, sys.stdout, indent=2)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
