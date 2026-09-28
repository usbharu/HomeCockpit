#!/usr/bin/env python3
"""Score expected MCP tool choices against a live Manager MCP server catalog."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent
SCENARIOS = ROOT / "mcp_tool_scenarios.json"
CLIENT = ROOT / "mcp_http_client.py"


def load_catalog(port: int) -> dict[str, str]:
    proc = subprocess.run(
        [sys.executable, str(CLIENT), "--port", str(port), "--list-tools"],
        check=True,
        capture_output=True,
        text=True,
    )
    payload = json.loads(proc.stdout)
    return {tool["name"]: tool.get("description", "") for tool in payload["tools"]}


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, default=28765)
    args = parser.parse_args()
    catalog = load_catalog(args.port)
    scenarios = json.loads(SCENARIOS.read_text())
    missing = [name for name in {s["tool"] for s in scenarios} if name not in catalog]
    if missing:
        print("Catalog missing expected tools:", ", ".join(sorted(missing)), file=sys.stderr)
        return 1
    empty = [name for name, desc in catalog.items() if not desc.strip()]
    if empty:
        print("Tools without description:", ", ".join(sorted(empty)), file=sys.stderr)
        return 1
    print(f"Live catalog OK: {len(catalog)} tools, {len(scenarios)} golden scenarios.")
    for scenario in scenarios:
        desc = catalog[scenario["tool"]]
        print(f"  - {scenario['id']}: {scenario['tool']}")
        if len(desc) < 40:
            print(f"    warning: short description ({len(desc)} chars)", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
