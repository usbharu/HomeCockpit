# DCS-BIOS DDI adapter catalog

This catalog is bundled with the DCS-BIOS adapter so Manager does not need a DCS
installation on the Manager machine. It contains the F-16C 50 MFD Left/MFD
Right and F/A-18C Left DDI/Right DDI push-button controls used by the DDI
mapping UI, including their input interfaces and output definitions. Brightness
and contrast controls, AMPCD, and UFC controls are intentionally excluded: the
current DDI firmware emits only `ControlValue::Button` events.

The source is the DCS-BIOS v0.11.7 `F-16C_50.json` and
`FA-18C_hornet.json` control references. When the adapter catalog is updated,
regenerate the filtered JSON from the pinned DCS-BIOS release and review the
control list before committing it.
