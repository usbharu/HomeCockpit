# F/A-18C DCS-BIOS control references

F/A-18C is the primary aircraft for the current HomeCockpit cockpit work, and this reference covers the Hornet's LEFT_DDI in detail. DCS-BIOS also supports other aircraft modules. For those aircraft, follow the same discovery and verification process with their own installed control reference; do not apply the identifiers or argument contracts below to another module.

Use this reference when a live DCS-BIOS stream identifies the aircraft as F/A-18C or when the task explicitly targets the Hornet's LEFT_DDI. The installed DCS-BIOS files and generated control reference remain authoritative because identifiers and input contracts can change between releases.

## Identify the active module first

1. Start with the read-only detector and record the export source, frame/record counts, and metadata:

   ```bash
   python3 .agents/skills/dcs-bios-connection/scripts/detect_dcs_bios.py \
     --host 239.255.50.10 --port 5010 --seconds 8
   ```

2. Decode `_ACFT_NAME` from the `MetadataStart` area of the export stream. It can be split across records or datagrams, so do not require the whole name in one UDP payload. The current F/A-18C module is named `FA-18C_hornet`; the upstream module also lists `EA-18G`, `FA-18E`, and `FA-18F` as aliases. Treat the exact name from the running mission as the observed fact.

3. On the DCS host, inspect the active Saved Games profile. Depending on the DCS-BIOS release, use one of these sources:

   - the generated `Scripts/DCS-BIOS/doc/control-reference.html`,
   - Bort or BIOSBuddy, or
   - the installed aircraft module, normally `Scripts/DCS-BIOS/lib/modules/aircraft_modules/FA-18C_hornet.lua`.

   Search the installed module rather than relying on a third-party example:

   ```bash
   rg -n 'LEFT_DDI_|define(PushButton|Potentiometer|Tumb)' \
     '<Saved Games>/<profile>/Scripts/DCS-BIOS'
   ```

   On Windows PowerShell, use `Select-String` with the same `LEFT_DDI_` pattern. If the DCS host is remote, obtain the reference there or use the exact release artifact; do not silently substitute the current upstream module for an older installation.

## Current upstream F/A-18C LEFT_DDI identifiers

These identifiers are a lookup aid and a starting point for the installed reference. Confirm them against the active release before sending:

| Control | Input contract in the current upstream module | Meaning |
| --- | --- | --- |
| `LEFT_DDI_BRT_SELECT` | `0`, `1`, `2` | OFF, NIGHT, DAY |
| `LEFT_DDI_BRT_CTL` | absolute `0..65535`; relative input may be supported by the installed release | Brightness control knob |
| `LEFT_DDI_CONT_CTL` | absolute `0..65535`; relative input may be supported by the installed release | Contrast control knob |
| `LEFT_DDI_PB_01` ... `LEFT_DDI_PB_20` | press `1`, release `0` | Left DDI pushbuttons 1–20 |
| `LEFT_DDI_HDG_SW` | release-specific rocker/switch contract | Heading set switch |
| `LEFT_DDI_CRS_SW` | release-specific rocker/switch contract | Course set switch |

The upstream F/A-18C module defines the three LEFT_DDI selectors/knobs and twenty pushbuttons in the `Left DDI` category. Its `defineTumb` and `definePotentiometer` calls are the source for the accepted ranges; do not infer an argument from the control name alone.

## Construct and verify a command

Use the repository's `dcs-bios-rs::import::ImportCommand` to validate and encode the line. DCS-BIOS import commands are LF-terminated plain text:

```text
LEFT_DDI_BRT_SELECT 1\n
LEFT_DDI_BRT_CTL 32768\n
LEFT_DDI_CONT_CTL 32768\n
LEFT_DDI_PB_01 1\n
LEFT_DDI_PB_01 0\n
```

The UDP send itself is not an acknowledgement. For each state-changing test:

1. Capture a short baseline export window.
2. Send exactly one verified command, or a press/release pair for a pushbutton.
3. Capture the next export frames and compare the corresponding output value or cockpit state.
4. Restore the original selector/knob state when the test is intended to be reversible.

For packed integer outputs, decode the little-endian word with the control reference's mask and shift. Do not assume that the module base address alone identifies a control: allocations can share a word, and addresses can change with the DCS-BIOS release. For transient pushbutton outputs, capture at the normal export rate and verify that the pressed bit appears and clears after the release.

## Evidence to record

Report the exact installed reference used, observed `_ACFT_NAME`, command destination, command line, and the export delta that proves the simulator accepted the operation. If the identifier is present in an upstream file but absent from the installed reference or no matching export change is observed, stop and resolve the release/aircraft mismatch instead of retrying guessed identifiers.

## Primary references

- [DCS-BIOS F/A-18C module](https://github.com/DCS-Skunkworks/dcs-bios/blob/main/Scripts/DCS-BIOS/lib/modules/aircraft_modules/FA-18C_hornet.lua)
- [DCS-BIOS developer guide](https://github.com/DCS-Skunkworks/dcs-bios/blob/main/Scripts/DCS-BIOS/doc/developerguide.adoc)
- [DCS-BIOS user guide](https://github.com/DCS-Skunkworks/dcs-bios/blob/main/Scripts/DCS-BIOS/doc/userguide.adoc)
