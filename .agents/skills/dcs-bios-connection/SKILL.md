---
name: dcs-bios-connection
description: Debug and implement DCS-BIOS connections in the HomeCockpit repository. Trigger for DCS-BIOS, DCS World Export.lua or BIOSConfig.lua, UDP 239.255.50.10:5010 or port 7778, multicast/unicast reception, control references, import commands, export packet parsing, or Manager connection failures. Use this skill for evidence-based diagnosis even when the user does not name the skill.
compatibility: Requires shell access and Python 3 for the bundled read-only detector.
---

# DCS-BIOS connection and protocol debugging

Keep the investigation centered on DCS-BIOS. Manager details are only an integration map.

## Read only what the task needs

- Read `references/dcs-bios-protocol.md` for endpoints, `BIOSConfig.lua`, frame layout, values, or commands.
- Read `references/connection-debug.md` for installation, LAN/multicast/unicast checks, or a no-packet diagnosis.
- Read `references/fa18c-controls.md` for F/A-18C control-reference lookup, LEFT_DDI identifiers, accepted import arguments, and export verification.
- Read `references/homecockpit.md` only when changing or interpreting Manager code.
- Run `scripts/detect_dcs_bios.py` for a live export check; do not write a new socket probe.

## Core model

DCS-BIOS is loaded by DCS World through `Export.lua`; it is not a LAN service with discovery or a handshake. DCS sends export datagrams, normally UDP multicast `239.255.50.10:5010`. Manager sends import commands to the DCS-BIOS listener, normally UDP `7778`. On separate machines, prefer unicast export to the Manager's LAN IP unless multicast routing is known to work.

Do not infer connectivity from a successful UDP send. Prove export reception with packet evidence, and prove a command by the simulator state or the resulting export value.

## Debugging sequence

1. Record whether DCS and Manager are on the same machine, the active DCS profile, both IPv4 addresses, and the actual `BIOSConfig.lua` endpoints.
2. On the DCS machine, verify `Scripts/DCS-BIOS/BIOS.lua`, the existing `Scripts/Export.lua`, its DCS-BIOS loader line, and a running mission. Preserve other exporters.
3. Listen on the Manager-side interface with the bundled detector. It reports datagram count, sources, logical frames, continuation chunks, and write records. A logical frame starts with four `0x55` bytes, but continuation datagrams need not.
4. Classify the result: no datagrams means export/config/network/firewall; datagrams with no valid logical records means endpoint or framing/parser; valid records with no Manager updates means Manager integration.
5. Configure Manager's export endpoint separately from its command destination. The expected local state is `stopped → connecting → listening → receiving`.
6. Send an import command only when explicitly requested, using the aircraft's current control reference. Never invent identifiers or arguments.

### Aircraft control lookup (F/A-18C primary)

F/A-18C is the primary aircraft for this repository's current cockpit work, but DCS-BIOS supports other aircraft modules as well. When the active aircraft is F/A-18C, read `references/fa18c-controls.md` before sending a command. For any other aircraft, apply the same lookup workflow with that aircraft's own installed control reference; never reuse F/A-18C identifiers or argument ranges without verification. Confirm the exact `_ACFT_NAME` and installed DCS-BIOS release first, then obtain the identifier and argument contract from the installed control reference or aircraft module definition. The upstream module is a useful cross-check, not a substitute for the installed release. For LEFT_DDI, verify both the identifier and its input range (`LEFT_DDI_BRT_SELECT`, `LEFT_DDI_BRT_CTL`, `LEFT_DDI_CONT_CTL`, or `LEFT_DDI_PB_01` through `LEFT_DDI_PB_20`) before constructing an LF-terminated import line.

## Code-change rules

Keep UDP chunking and multicast membership, `SO_REUSEADDR`, the four-byte sync marker, little-endian address/length fields, multiple records per frame, cross-datagram parser state, and malformed-frame recovery. Add a regression test for parser or command changes. For Manager changes, inspect the files listed in `references/homecockpit.md` and run the targeted Rust tests plus `manager/npm run build` when applicable.

## Report

Separate observed facts from assumptions. Report topology, export and command endpoints, detector evidence (count/rate/source/valid records), Manager state or exact error, and one next safe action. Do not send state-changing commands during discovery-only work.
