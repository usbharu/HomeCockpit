# DCS-BIOS connection runbook

Use this only for installation and connection diagnosis. Keep protocol details in `dcs-bios-protocol.md`.

## DCS-side prerequisites

On the DCS machine, use the active Saved Games profile (`DCS` or `DCS.openbeta`):

```text
Saved Games/<profile>/Scripts/DCS-BIOS/BIOS.lua
Saved Games/<profile>/Scripts/Export.lua
```

Install or update by copying the upstream `DCS-BIOS` folder into the profile's `Scripts` folder.
`Export.lua` must load DCS-BIOS while preserving existing exporters. The upstream loader is normally:

```lua
dofile(lfs.writedir() .. [[Scripts\DCS-BIOS\BIOS.lua]])
```

Do not replace or reformat an existing `Export.lua` blindly. Load a mission before expecting aircraft-specific metadata. Check DCS-BIOS logs and the DCS log if the export hook appears not to run.

## Observe before changing code

Run this on the host that should receive export data:

```bash
python3 .agents/skills/dcs-bios-connection/scripts/detect_dcs_bios.py \
  --host 239.255.50.10 --port 5010 --seconds 8
```

For a configured unicast export, use the local receiving interface as the listener and the configured address as the report label:

```bash
python3 .agents/skills/dcs-bios-connection/scripts/detect_dcs_bios.py \
  --host <MANAGER_IP> --port 5010 --interface <MANAGER_IP> --seconds 8
```

The detector is read-only. It reports datagrams, source IP/port, frame-start chunks, continuation chunks, logical frames, and record counts. It carries parser state across UDP datagrams, so a continuation chunk without `55 55 55 55` is not automatically invalid. Add `--show-hex` when comparing framing. Treat these results as the primary evidence:

| Result | Next check |
| --- | --- |
| No datagrams | DCS `Export.lua`/mission, actual `BIOSConfig.lua`, firewall, route, and interface |
| Datagrams but no valid records | Wrong port/source, packet loss, non-DCS traffic, or framing/parser mismatch |
| Valid frames | Network path works; inspect Manager bind, decode, memory-map, and UI handling |
| Manager `listening` but not `receiving` | Socket exists but no fresh datagram reaches it |
| Manager `error` | Read the exact bind/join error and inspect the process using UDP `5010` |

On macOS, the detector can observe the multicast group while Manager owns a wildcard bind. If it still reports `Address already in use`, inspect rather than killing an unknown process:

```bash
lsof -nP -iUDP:5010
tcpdump -ni <interface> udp port 5010
```

On Linux use `ip -4 addr`, `ip route`, and `sudo tcpdump -ni <interface> udp port 5010`. On Windows use `Get-NetIPAddress -AddressFamily IPv4`, `Get-NetUDPEndpoint -LocalPort 5010`, and Wireshark.

## Same host versus separate hosts

- Same host: join `239.255.50.10:5010`; send commands to `127.0.0.1:7778`.
- Separate hosts: first prefer `send_address=<MANAGER_IP>` in the existing DCS-BIOS UDP configuration; send commands to `<DCS_IP>:7778`. Use multicast only when the router, Wi-Fi, and firewalls are known to pass it.

Do not confuse the export source's ephemeral UDP source port with the command listener port. A packet capture on the DCS host proves sending, not receipt on the Manager host.

## Command test

Only send a command when the user explicitly asks for an operational test. Use the aircraft's current control reference and send exactly one LF-terminated line. A local UDP send is not an acknowledgement; verify the cockpit or a matching export update. If the command has no effect, check the DCS PC address, port `7778`, active aircraft module, identifier, argument, and firewall before changing the export parser.
