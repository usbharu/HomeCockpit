# Pico 2 bring-up notes

This directory contains a minimal Rust blinky project for Raspberry Pi Pico 2 (`RP235x`).

## What we confirmed

- The board showed up on Windows as a CMSIS-DAP probe.
- Old `probe-rs` could see the probe, but could not properly talk to the target.
- Updating `probe-rs` fixed `RP235x` support.
- Flashing succeeded over SWD and the onboard LED blinked.

## Symptoms we saw first

With an older `probe-rs`, the connected board looked like this:

- `Debugprobe on Pico (CMSIS-DAP)`
- USB VID/PID `2e8a:000c`

But target detection failed with errors like:

```text
Debug port not supported: Unsupported version (DPv3)
```

That turned out to be a tool version issue, not a bad board.

## Required tool update

We updated `probe-rs` on Windows with the official installer script:

```powershell
irm https://github.com/probe-rs/probe-rs/releases/latest/download/probe-rs-tools-installer.ps1 | iex
```

After that:

```powershell
probe-rs --version
probe-rs chip list | Select-String RP235
probe-rs list
```

Expected result:

- `probe-rs` is updated
- `RP235x` appears in the chip list
- the Pico is listed as a CMSIS-DAP debug probe

## Verifying the target

This was the command that successfully identified the Pico 2 target:

```powershell
probe-rs info --probe 2e8a:000c-0:E661AC8863428A39 --protocol swd --chip RP235x --speed 100 --connect-under-reset
```

Important detail:

- `info` worked best with `--connect-under-reset`
- flashing did **not** work reliably with `--connect-under-reset`

When successful, the output included:

- `Debug Port: DPv3`
- `PARTNO: Cortex-M33`
- `RP235x CoreSight ROM`

## Building the blinky firmware

This project is a standalone minimal Rust firmware for Pico 2.

Build it with:

```powershell
cd C:\Users\haruj\Documents\HomeCockpit\firmware\pico2_blinky
cargo build
```

Key configuration:

- target: `thumbv8m.main-none-eabihf`
- chip: `RP235x`
- LED pin: `GP25`

## Flashing the board

`cargo run` built successfully, but the default runner using `--connect-under-reset` timed out during flashing on this setup.

The command that actually worked was:

```powershell
probe-rs download --probe 2e8a:000c-0:E661AC8863428A39 --chip RP235x --protocol swd --speed 100 --disable-double-buffering --verify target\thumbv8m.main-none-eabihf\debug\pico2_blinky
```

Then reset the board:

```powershell
probe-rs reset --probe 2e8a:000c-0:E661AC8863428A39 --chip RP235x --protocol swd --speed 100
```

## Expected behavior

The firmware toggles `GP25` every 500 ms.

If the board is a standard Pico 2, the onboard LED should blink.

If the board is a Pico 2 W:

- the onboard LED is not on `GP25`
- flashing may still succeed
- the onboard LED may still appear not to blink

## Files in this project

- `Cargo.toml`: standalone crate definition
- `.cargo/config.toml`: `RP235x` target and runner settings
- `memory.x`: RP235x memory layout
- `build.rs`: copies `memory.x` into the linker output dir
- `src/main.rs`: the actual blinky firmware

## Quick command summary

```powershell
irm https://github.com/probe-rs/probe-rs/releases/latest/download/probe-rs-tools-installer.ps1 | iex

probe-rs list
probe-rs chip list | Select-String RP235
probe-rs info --probe 2e8a:000c-0:E661AC8863428A39 --protocol swd --chip RP235x --speed 100 --connect-under-reset

cd C:\Users\haruj\Documents\HomeCockpit\firmware\pico2_blinky
cargo build
probe-rs download --probe 2e8a:000c-0:E661AC8863428A39 --chip RP235x --protocol swd --speed 100 --disable-double-buffering --verify target\thumbv8m.main-none-eabihf\debug\pico2_blinky
probe-rs reset --probe 2e8a:000c-0:E661AC8863428A39 --chip RP235x --protocol swd --speed 100
```
