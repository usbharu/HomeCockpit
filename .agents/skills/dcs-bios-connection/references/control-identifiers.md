# DCS-BIOS control identifier lookup

Use this procedure to find identifiers and input contracts for any DCS-BIOS aircraft module. F/A-18C is the primary aircraft for the current HomeCockpit work, so it is used for a short example near the end. The example is not the scope of the procedure: LEFT_DDI and its identifiers must not be assumed when the requested aircraft or control is different.

The installed DCS-BIOS release is authoritative. An identifier can exist in one module or release and be absent, renamed, or assigned a different argument contract in another.

## 1. Identify the aircraft module and release

1. If a live export stream is available, run the bundled read-only detector and record its source, frame/record counts, and metadata:

   ~~~bash
   python3 .agents/skills/dcs-bios-connection/scripts/detect_dcs_bios.py \
     --host 239.255.50.10 --port 5010 --seconds 8
   ~~~

2. Decode `_ACFT_NAME` from the `MetadataStart` area. The value can be split across records or datagrams, so do not require the complete name in one UDP payload. Treat the value observed in the running mission as the active module name.

3. Record the DCS-BIOS release or source commit. Prefer the version reported by the DCS host, the installed package, or the project's lock/configuration data. If that information is unavailable, label the source as unpinned and do not claim that a lookup is verified for the running installation.

The terminal running the agent and the DCS host are separate concerns. DCS may be installed on another machine, or may not be installed on either machine during an offline lookup. Do not assume that the agent terminal has a Saved Games folder or an installed `Scripts/DCS-BIOS` tree.

## 2. Obtain the matching control reference

### DCS host files are available

Use the files from the active DCS Saved Games profile, preferably one of these sources:

- generated `Scripts/DCS-BIOS/doc/control-reference.html`;
- the installed aircraft module under `Scripts/DCS-BIOS/lib/modules/aircraft_modules/`; or
- a local control-reference tool such as Bort or BIOSBuddy, treated as a convenience view of the same release.

### DCS is not installed on the agent terminal

Fetch the definition from the official [DCS-BIOS GitHub repository](https://github.com/DCS-Skunkworks/dcs-bios). Pin the checkout to the exact tag or commit whenever possible; the `main` branch is only a current-source cross-check.

For a local, searchable checkout:

~~~bash
git clone --filter=blob:none https://github.com/DCS-Skunkworks/dcs-bios.git dcs_bios_repo
git -C dcs_bios_repo fetch --tags --force
git -C dcs_bios_repo switch --detach <tag-or-commit>
git -C dcs_bios_repo ls-tree -r --name-only <tag-or-commit> -- \
  Scripts/DCS-BIOS/lib/modules/aircraft_modules
~~~

When only one source file is needed, use the raw file at the pinned ref:

~~~bash
curl -fsSL \
  https://raw.githubusercontent.com/DCS-Skunkworks/dcs-bios/<tag-or-commit>/Scripts/DCS-BIOS/lib/modules/aircraft_modules/<module-file>.lua
~~~

The repository's [release page](https://github.com/DCS-Skunkworks/dcs-bios/releases) is also an official way to obtain a versioned DCS-BIOS archive. If the exact installed version is unknown, use the GitHub source only as a lookup aid, record that limitation, and verify again against the DCS host before sending a state-changing command.

## 3. Search the module definition

After selecting the module file, search for the requested control name or a meaningful prefix. Search the definition rather than guessing from cockpit labels:

~~~bash
rg -n 'define[A-Za-z_]+\(|<identifier-or-prefix>' \
  dcs_bios_repo/Scripts/DCS-BIOS/lib/modules/aircraft_modules/<module-file>.lua
~~~

Read the complete definition and record:

- the exact case-sensitive identifier;
- control type, such as push button, toggle, selector, potentiometer, or display output;
- accepted import argument values and whether a value is absolute or relative;
- output address, mask, shift, or string length when decoding export data; and
- the source ref and file path used for the lookup.

Do not infer an argument range from the identifier name or from a module base address. Definitions can share packed words, and allocations/contracts can change between releases.

## 4. F/A-18C example only

If the live metadata reports `FA-18C_hornet`, the same generic search can be narrowed to the Hornet module and a requested prefix. For example, this finds representative LEFT_DDI definitions without making LEFT_DDI the general lookup target:

~~~bash
rg -n 'LEFT_DDI_BRT_SELECT|LEFT_DDI_PB_01|define[A-Za-z_]+\(' \
  dcs_bios_repo/Scripts/DCS-BIOS/lib/modules/aircraft_modules/FA-18C_hornet.lua
~~~

The result may show an example such as `LEFT_DDI_BRT_SELECT` accepting selector values or `LEFT_DDI_PB_01` accepting a press/release pair. Those values must still be read from the selected release's definition. For another aircraft, replace the module file and search term; do not copy the F/A-18C names or ranges.

## 5. Construct and verify a command

Use the repository's `dcs-bios-rs::import::ImportCommand` to validate and encode a verified line. DCS-BIOS import commands are plain text terminated by LF:

~~~text
<identifier> <verified-argument>\n
~~~

The UDP send is not an acknowledgement. For every state-changing test:

1. Capture a short baseline export window.
2. Send exactly one verified command, or a documented press/release pair.
3. Compare the next export frames or simulator state with the baseline.
4. Restore the original state when the test is intended to be reversible.

For packed output values, decode the little-endian data using the selected definition's address, mask, and shift. For transient controls, capture at the normal export rate and verify both the asserted and cleared states.

## Evidence to record

Report the observed `_ACFT_NAME`, DCS-BIOS version/tag/commit, reference source and path, exact identifier and argument contract, command destination, command line, and export/state change. If a GitHub definition does not match the installed DCS host or no corresponding export change is observed, stop and resolve the version/module mismatch instead of retrying guessed identifiers.

## Primary references

- [DCS-BIOS GitHub repository](https://github.com/DCS-Skunkworks/dcs-bios)
- [DCS-BIOS releases](https://github.com/DCS-Skunkworks/dcs-bios/releases)
- [DCS-BIOS F/A-18C module example](https://github.com/DCS-Skunkworks/dcs-bios/blob/main/Scripts/DCS-BIOS/lib/modules/aircraft_modules/FA-18C_hornet.lua)
