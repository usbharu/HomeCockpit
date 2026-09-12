# HomeCockpit integration map

Load this file only for a Manager code or UI task. DCS-BIOS protocol behavior remains authoritative over this project note.

## Relevant files

| Concern | File |
| --- | --- |
| UDP listener, status, Tauri commands, command transport | `manager/src-tauri/src/lib.rs` |
| Export frame parser and memory map | `dcs-bios-rs/src/lib.rs` |
| Import command validation/encoding | `dcs-bios-rs/src/import.rs` |
| Manager config/status types and defaults | `manager/src/lib/manager-types.ts` |
| Settings/status UI | `manager/src/components/tabs/software-settings.tsx`, `manager/src/components/tabs/status-page.tsx` |

## Current Manager defaults

```text
exportHost       = 239.255.50.10
exportPort       = 5010
commandHost      = 127.0.0.1
commandPort      = 7778
commandTransport = udp
```

`exportHost` identifies the multicast group or configured export destination; it is not necessarily the local bind address. `commandHost` is where import commands are sent. For a remote DCS machine, use its LAN IP for `commandHost`.

## Current behavior to preserve

- `bind_export_socket` binds `0.0.0.0:<exportPort>`, sets `SO_REUSEADDR`, joins a parsed multicast group on the unspecified interface, and uses a 500 ms read timeout.
- The listener counts received datagrams, passes each UDP datagram to `dcs-bios-rs`, updates the shared memory map, and emits status/log/frame events.
- The Rust packet iterator requires sync at offset zero and parses multiple records, but it does not reassemble DCS-BIOS continuation chunks and is not a strict validator: malformed or truncated trailing data can simply stop iteration without an error. `receiving` can therefore mean packets arrive while some export data is missing; compare decoded values and logs, not packet count alone.
- `send_dcsbios_command` sends a newline-terminated import line over UDP or TCP. UDP has no response path.
- `update_dcsbios_config` replaces runtime configuration and restarts device endpoint listeners, but does not stop/restart the DCS-BIOS export listener. Stop/start DCS-BIOS explicitly after changing its endpoint; settings are not persisted across a restart.

## Verification after code changes

If a captured DCS-BIOS update is split across UDP chunks, fix this boundary in Manager or the adapter by buffering/reassembling the logical stream; do not solve it by ignoring packets without a sync prefix.

Add or update colocated tests for framing, cross-datagram reassembly, malformed data, memory-map updates, and command encoding. Run:

```bash
cargo test --manifest-path manager/src-tauri/Cargo.toml
cd manager && npm run build
```

Do not change Manager code to report `receiving` without a real datagram. First prove the network path with the bundled detector.
