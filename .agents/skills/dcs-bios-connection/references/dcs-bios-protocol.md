# DCS-BIOS protocol essentials

This is the protocol reference to load before debugging a packet, socket, value, or command. Verify release-specific details against the installed `BIOSConfig.lua` and the official DCS-BIOS guides.

## Transport

| Direction | Default endpoint | Meaning |
| --- | --- | --- |
| DCS-BIOS → client | UDP multicast `239.255.50.10:5010` | Export stream; DCS sends datagrams |
| client → DCS-BIOS | UDP `*:7778` | Import commands; DCS-BIOS listens for text lines |

The default export is local to the DCS host. There is no discovery packet, registration, or UDP handshake: a client joins the multicast group and observes traffic. On another machine, multicast works only if the network routes it; unicast export to the Manager's LAN IP is usually simpler. In the installed `BIOSConfig.lua`, change the existing UDP entry rather than creating a second listener:

```lua
udp_config = {
    {
        send_address = "<MANAGER_IP>",
        send_port = 5010,
        receive_address = "*",
        receive_port = 7778,
    },
}
```

The exact table can differ by release, so treat this as the intended mapping, not a replacement file. DCS-BIOS also has an optional TCP path on `7778`; it is not a substitute for the UDP export datagrams.

## Export stream format

The export protocol is a logical byte stream transported in UDP chunks. A logical update is:

```text
frame   = 55 55 55 55 record*
record  = address:u16-le length:u16-le payload[length]
```

The current upstream sender packs queued data into UDP payloads up to 1460 bytes. Therefore a logical update can span multiple UDP datagrams, and only the chunk containing the frame start is guaranteed to begin with the four-byte sync sequence. A receiver must carry parser state across datagrams; a datagram boundary is not a frame boundary.

Rules that matter in a debugger:

- Use `0x55 0x55 0x55 0x55` to start or re-synchronize a logical frame. Do not reject a datagram merely because it does not start with sync when it is a continuation of the current frame.
- Decode both header fields as unsigned little-endian 16-bit values.
- Accept multiple records after one sync prefix and carry an incomplete header or payload into the next UDP chunk.
- Export addresses and lengths are even for the 16-bit memory writes. Reject incomplete headers, odd addresses or lengths when strict validation is desired, and truncated payloads.
- UDP has no retransmission. After packet loss or a malformed record, discard bytes until the next sync sequence and report the lost/incomplete frame.
- If reading the optional TCP stream, use the same stream buffering: one read can split or contain multiple frames.

Example:

```text
55 55 55 55  00 10 04 00  41 2d 31 30
              address=0x1000 length=4 payload="A-10"
```

The memory map is updated from each record. An integer control is commonly derived from a little-endian word:

```text
value = (u16_le(word) & mask) >> shift
```

Strings use a fixed address and maximum length; decode only the requested range and treat padding/termination according to the aircraft control reference. Long string writes may be partial, so expose the accumulated string at the next frame-sync boundary rather than showing an intermediate value. Mission metadata includes `_ACFT_NAME` under `MetadataStart`; it is normally meaningful after a mission starts and is cleared when the mission ends. The stream is typically about 30 updates per second, but packet rate depends on the active mission and changed values.

## Import command format

An import command is plain text terminated by LF:

```text
<CONTROL_IDENTIFIER> <ARGUMENT>\n
```

The identifier and accepted argument are aircraft-module-specific. Obtain them from the generated `control-reference.html`, Bort, BIOSBuddy, or a supplied reference; do not guess. UDP has no acknowledgement, so confirm the cockpit effect or observe the corresponding export value. TCP only changes delivery semantics; it does not make an unknown control valid.

## Socket facts

For multicast reception, set `SO_REUSEADDR` before binding the local UDP port, bind the local port, then join `239.255.50.10` on the intended interface. Binding `0.0.0.0:<port>` is portable; macOS may need a multicast listener to bind the group address when another wildcard listener already owns the port. For unicast, bind the local port without joining a multicast group. A firewall must permit inbound UDP `5010` on the receiving interface and inbound UDP `7778` on the DCS machine for commands.

## Sources

- [DCS-BIOS README](https://github.com/DCS-Skunkworks/dcs-bios)
- [Developer Guide](https://github.com/DCS-Skunkworks/dcs-bios/blob/main/Scripts/DCS-BIOS/doc/developerguide.adoc)
- [User Guide](https://github.com/DCS-Skunkworks/dcs-bios/blob/main/Scripts/DCS-BIOS/doc/userguide.adoc)
- [Current BIOSConfig.lua](https://raw.githubusercontent.com/DCS-Skunkworks/dcs-bios/main/Scripts/DCS-BIOS/BIOSConfig.lua)
- [Current BIOSStateMachine.lua](https://raw.githubusercontent.com/DCS-Skunkworks/dcs-bios/main/Scripts/DCS-BIOS/lib/BIOSStateMachine.lua)
- [Current ConnectionManager.lua](https://raw.githubusercontent.com/DCS-Skunkworks/dcs-bios/main/Scripts/DCS-BIOS/lib/ConnectionManager.lua)
