#!/usr/bin/env python3
"""Generate upper_panel_ddi_main_board.kicad_sch from firmware pin assignments."""

from __future__ import annotations

import re
import uuid
from pathlib import Path
import subprocess

ROOT_UUID = "a1b2c3d4-e5f6-7890-abcd-ef1234567890"
PROJECT = "upper_panel_ddi_main_board"

PICO_AT = (191.77, 74.93)
ALL_PICO_PINS = [str(n) for n in range(1, 41)]

GPIO_TO_PIN: dict[int, int] = {
    0: 1,
    1: 2,
    2: 4,
    3: 5,
    4: 6,
    5: 7,
    6: 9,
    7: 10,
    8: 11,
    9: 12,
    10: 14,
    11: 15,
    12: 16,
    13: 17,
    14: 19,
    15: 20,
    16: 21,
    17: 22,
    18: 24,
    19: 25,
    20: 26,
    21: 27,
    22: 29,
    27: 32,
    28: 34,
}


def _load_pico_pin_locals() -> dict[int, tuple[float, float, int, float, str]]:
    board_dir = Path(__file__).resolve().parent
    text = (board_dir / "embedded_kicad10_RaspberryPi_Pico.txt").read_text(encoding="utf-8")
    unit = text.split("RaspberryPi_Pico_1_1", 1)[1]
    pins: dict[int, tuple[float, float, int, float, str]] = {}
    pattern = re.compile(
        r"\(pin [^\)]+\(at (-?\d+\.?\d*) (-?\d+\.?\d*) (\d+)\)[\s\S]*?"
        r"\(length (\d+\.?\d*)\)[\s\S]*?"
        r'\(name "([^"]*)"[\s\S]*?\(number "(\d+)"'
    )
    for match in pattern.finditer(unit):
        lx, ly, angle, length, name, num = match.groups()
        pins[int(num)] = (float(lx), float(ly), int(angle), float(length), name)
    return pins


PICO_PIN_LOCAL = _load_pico_pin_locals()
PICO_POWER_PINS = {3, 8, 13, 18, 23, 28, 33, 35, 36, 38, 39, 40}

# Conn_01x08_Socket @ J1_AT with mirror Y; Conn_01x05_Socket @ J2_AT (KiCad 10 geometry).
def j1_pin_xy(row: int) -> tuple[float, float]:
    px, py = J1_AT
    local_y = 7.62 - row * 2.54
    return (snap_mm(px + 5.08), snap_mm(py - local_y))


def j2_pin_xy(col: int) -> tuple[float, float]:
    """Conn_01x05_Socket passive pin anchor at J2_AT (ERC: x=px-5.08)."""
    px, py = J2_AT
    pin_number = col + 1
    return (snap_mm(px - 5.08), snap_mm(py + (pin_number - 3) * 2.54))


def j1_symbol_pin_xy(row: int) -> tuple[float, float]:
    px, py = J1_AT
    pin_number = row + 1
    local_y = 7.62 - (pin_number - 1) * 2.54
    return pin_end(px, py, -5.08, -local_y, 0)


def j2_symbol_pin_xy(col: int) -> tuple[float, float]:
    px, py = J2_AT
    pin_number = col + 1
    local_y = 5.08 - (pin_number - 1) * 2.54
    return pin_end(px, py, -5.08, local_y, 0)


J1_AT = (54.61, 39.37)
J2_AT = (168.91, 132.08)
J3_AT = (254.0, 74.93)


def uid() -> str:
    return str(uuid.uuid4())


def pin_end(px: float, py: float, x: float, y: float, angle: int, length: float = 3.81) -> tuple[float, float]:
    if angle == 0:
        return (snap_mm(px + x + length), snap_mm(py + y))
    if angle == 180:
        return (snap_mm(px + x - length), snap_mm(py + y))
    if angle == 90:
        return (snap_mm(px + x), snap_mm(py + y + length))
    if angle == 270:
        return (snap_mm(px + x), snap_mm(py + y - length))
    raise ValueError(angle)


def snap_mm(value: float) -> float:
    return round(value / 0.254) * 0.254


def pico_pin_world(pin_number: int) -> tuple[float, float]:
    lx, ly, _angle, _length, _ = PICO_PIN_LOCAL[pin_number]
    px, py = PICO_AT
    return (snap_mm(px + lx), snap_mm(py - ly))


def pico_pin_xy(gpio: int) -> tuple[float, float]:
    return pico_pin_world(GPIO_TO_PIN[gpio])


def pico_power_xy() -> dict[str, tuple[float, float]]:
    return {
        "VBUS": pico_pin_world(40),
        "VSYS": pico_pin_world(39),
        "3V3": pico_pin_world(36),
        "GND": pico_pin_world(3),
        "ADC_VREF": pico_pin_world(35),
    }


def wire(x1: float, y1: float, x2: float, y2: float) -> str:
    x1, y1, x2, y2 = snap_mm(x1), snap_mm(y1), snap_mm(x2), snap_mm(y2)
    return f"""
\t(wire
\t\t(pts
\t\t\t(xy {x1} {y1}) (xy {x2} {y2})
\t\t)
\t\t(stroke
\t\t\t(width 0)
\t\t\t(type default)
\t\t)
\t\t(uuid "{uid()}")
\t)"""


def junction(x: float, y: float) -> str:
    return f"""
\t(junction
\t\t(at {x} {y})
\t\t(diameter 0)
\t\t(color 0 0 0 0)
\t\t(uuid "{uid()}")
\t)"""


def no_connect(x: float, y: float) -> str:
    return f"""
\t(no_connect
\t\t(at {x} {y})
\t\t(uuid "{uid()}")
\t)"""


def text_note(x: float, y: float, lines: list[str]) -> str:
    body = "\\n".join(lines)
    return f"""
\t(text "{body}"
\t\t(exclude_from_sim yes)
\t\t(at {x} {y} 0)
\t\t(effects
\t\t\t(font
\t\t\t\t(size 1.27 1.27)
\t\t\t)
\t\t\t(justify left top)
\t\t)
\t\t(uuid "{uid()}")
\t)"""


def symbol_instance(
    lib_id: str,
    ref: str,
    value: str,
    at: tuple[float, float],
    footprint: str,
    pin_numbers: list[str],
    mirror_y: bool = False,
    angle: int = 0,
) -> str:
    pin_lines = "\n".join(f'\t\t(pin "{n}"\n\t\t\t(uuid "{uid()}")\n\t\t)' for n in pin_numbers)
    mirror = "\n\t\t(mirror y)" if mirror_y else ""
    return f"""
\t(symbol
\t\t(lib_id "{lib_id}")
\t\t(at {at[0]} {at[1]} {angle}){mirror}
\t\t(unit 1)
\t\t(exclude_from_sim no)
\t\t(in_bom yes)
\t\t(on_board yes)
\t\t(dnp no)
\t\t(uuid "{uid()}")
\t\t(property "Reference" "{ref}"
\t\t\t(at {at[0] - 5} {at[1]} 0)
\t\t\t(effects
\t\t\t\t(font
\t\t\t\t\t(size 1.27 1.27)
\t\t\t\t)
\t\t\t\t(justify right)
\t\t\t)
\t\t)
\t\t(property "Value" "{value}"
\t\t\t(at {at[0] - 5} {at[1] + 2.54} 0)
\t\t\t(effects
\t\t\t\t(font
\t\t\t\t\t(size 1.27 1.27)
\t\t\t\t)
\t\t\t\t(justify right)
\t\t\t)
\t\t)
\t\t(property "Footprint" "{footprint}"
\t\t\t(at {at[0]} {at[1]} 0)
\t\t\t(effects
\t\t\t\t(font
\t\t\t\t\t(size 1.27 1.27)
\t\t\t\t)
\t\t\t\t(hide yes)
\t\t\t)
\t\t)
\t\t(property "Datasheet" "~"
\t\t\t(at {at[0]} {at[1]} 0)
\t\t\t(effects
\t\t\t\t(font
\t\t\t\t\t(size 1.27 1.27)
\t\t\t\t)
\t\t\t\t(hide yes)
\t\t\t)
\t\t)
{pin_lines}
\t\t(instances
\t\t\t(project "{PROJECT}"
\t\t\t\t(path "/{ROOT_UUID}"
\t\t\t\t\t(reference "{ref}")
\t\t\t\t\t(unit 1)
\t\t\t\t)
\t\t\t)
\t\t)
\t)"""


def power_symbol(ref: str, lib_id: str, at: tuple[float, float], angle: int = 0) -> str:
    value = lib_id.split(":")[1]
    return f"""
\t(symbol
\t\t(lib_id "{lib_id}")
\t\t(at {at[0]} {at[1]} {angle})
\t\t(unit 1)
\t\t(exclude_from_sim no)
\t\t(in_bom yes)
\t\t(on_board yes)
\t\t(dnp no)
\t\t(fields_autoplaced yes)
\t\t(uuid "{uid()}")
\t\t(property "Reference" "{ref}"
\t\t\t(at {at[0] + 2} {at[1] - 2} 0)
\t\t\t(effects
\t\t\t\t(font
\t\t\t\t\t(size 1.27 1.27)
\t\t\t\t)
\t\t\t\t(hide yes)
\t\t\t)
\t\t)
\t\t(property "Value" "{value}"
\t\t\t(at {at[0]} {at[1]} 0)
\t\t\t(effects
\t\t\t\t(font
\t\t\t\t\t(size 1.27 1.27)
\t\t\t\t)
\t\t\t\t(hide yes)
\t\t\t)
\t\t)
\t\t(property "Footprint" ""
\t\t\t(at {at[0]} {at[1]} 0)
\t\t\t(effects
\t\t\t\t(font
\t\t\t\t\t(size 1.27 1.27)
\t\t\t\t)
\t\t\t\t(hide yes)
\t\t\t)
\t\t)
\t\t(pin "1"
\t\t\t(uuid "{uid()}")
\t\t)
\t\t(instances
\t\t\t(project "{PROJECT}"
\t\t\t\t(path "/{ROOT_UUID}"
\t\t\t\t\t(reference "{ref}")
\t\t\t\t\t(unit 1)
\t\t\t\t)
\t\t\t)
\t\t)
\t)"""


def extract_embedded_symbol(block: str, name: str) -> str:
    marker = f'(symbol "{name}"'
    start = block.index(marker)
    depth = 0
    for i in range(start, len(block)):
        ch = block[i]
        if ch == "(":
            depth += 1
        elif ch == ")":
            depth -= 1
            if depth == 0:
                return block[start : i + 1]
    raise RuntimeError(f"unterminated symbol {name}")


def load_embedded_lib_symbols() -> str:
    board_dir = Path(__file__).resolve().parent
    blocks = [
        (board_dir / "embedded_from_button_Conn_01x08_Pin.txt").read_text(encoding="utf-8"),
        (board_dir / "embedded_kicad10_Conn_01x05_Socket.txt").read_text(encoding="utf-8"),
        (board_dir / "embedded_kicad10_RaspberryPi_Pico.txt").read_text(encoding="utf-8"),
        (board_dir / "embedded_kicad10_power_GND.txt").read_text(encoding="utf-8"),
        (board_dir / "embedded_kicad10_power_plus5V.txt").read_text(encoding="utf-8"),
        (board_dir / "embedded_kicad10_power_plus3V3.txt").read_text(encoding="utf-8"),
    ]
    body = "".join(f"\t\t{b.strip()}\n" for b in blocks)
    return f"(lib_symbols\n{body}\t)\n"


def global_label(name: str, x: float, y: float, orient: int = 180) -> str:
    return f"""
\t(global_label "{name}"
\t\t(at {x} {y} {orient})
\t\t(fields_autoplaced yes)
\t\t(effects
\t\t\t(font
\t\t\t\t(size 1.27 1.27)
\t\t\t)
\t\t\t(justify left)
\t\t)
\t\t(uuid "{uid()}")
\t\t(property "Intersheetref" "{name}"
\t\t\t(at {x} {y} {orient})
\t\t\t(effects
\t\t\t\t(font
\t\t\t\t\t(size 1.27 1.27)
\t\t\t\t)
\t\t\t\t(justify left)
\t\t\t\t(hide yes)
\t\t\t)
\t\t)
\t)"""


def net_label(name: str, x: float, y: float) -> str:
    return f"""
\t(label "{name}"
\t\t(at {x} {y} 0)
\t\t(fields_autoplaced yes)
\t\t(effects
\t\t\t(font
\t\t\t\t(size 1.27 1.27)
\t\t\t)
\t\t\t(justify right)
\t\t)
\t\t(uuid "{uid()}")
\t)"""


def main() -> None:
    chunks: list[str] = []

    chunks.append(
        text_note(
            25.4,
            25.4,
            [
                "Upper Panel DDI main board (perfboard) — firmware: upper_panel_ddi",
                "Matrix: 8x5 (GP2-9 rows out, GP10-14 cols in, pulldown in FW)",
                "IMCP: USB CDC on Pico Micro-USB (115200 logical); GP0/GP1 UART NC",
                "Power: USB VBUS; max_power 100 mA in firmware USB descriptor",
                "J1 ROW0-7 -> GP2-9 | J2 COL0-4 -> GP10-14 | GND common harness",
            ],
        )
    )
    chunks.append(
        text_note(
            25.4,
            160.0,
            [
                "J3: wire to Pico module debug pads (SWDIO/SWCLK/GND)",
                "Not represented on RaspberryPi_Pico symbol pins.",
            ],
        )
    )

    power_xy = pico_power_xy()
    gnd_net = power_xy["GND"]
    vbus_net = power_xy["VBUS"]
    vsys_net = power_xy["VSYS"]
    adc_ref = power_xy["ADC_VREF"]
    mcu_3v3 = power_xy["3V3"]

    chunks.append(
        symbol_instance(
            "MCU_Module:RaspberryPi_Pico",
            "A1",
            "RaspberryPi_Pico",
            PICO_AT,
            "Module:RaspberryPi_Pico_Common_THT",
            ALL_PICO_PINS,
        )
    )

    chunks.append(junction(gnd_net[0], gnd_net[1]))
    chunks.append(wire(gnd_net[0], gnd_net[1], gnd_net[0] + 2.54, gnd_net[1]))
    chunks.append(power_symbol("#PWR01", "power:GND", (gnd_net[0] + 2.54, gnd_net[1])))
    chunks.append(junction(vbus_net[0], vbus_net[1]))
    chunks.append(wire(vbus_net[0], vbus_net[1], vbus_net[0] + 2.54, vbus_net[1]))
    chunks.append(power_symbol("#PWR02", "power:+5V", (vbus_net[0] + 2.54, vbus_net[1])))
    chunks.append(wire(vsys_net[0], vsys_net[1], vbus_net[0], vbus_net[1]))
    chunks.append(junction(mcu_3v3[0], mcu_3v3[1]))
    chunks.append(wire(mcu_3v3[0], mcu_3v3[1], mcu_3v3[0] + 2.54, mcu_3v3[1]))
    chunks.append(power_symbol("#PWR03", "power:+3V3", (mcu_3v3[0] + 2.54, mcu_3v3[1])))
    chunks.append(wire(adc_ref[0], adc_ref[1], mcu_3v3[0], mcu_3v3[1]))
    agnd = pico_pin_world(33)
    chunks.append(junction(agnd[0], agnd[1]))
    chunks.append(no_connect(agnd[0], agnd[1]))

    chunks.append(
        symbol_instance(
            "Connector:Conn_01x08_Pin",
            "J1",
            "ROW0-7",
            J1_AT,
            "Connector_JST:JST_ZH_B8B-ZR_1x08_P1.50mm_Vertical",
            [str(n) for n in range(1, 9)],
            mirror_y=False,
            angle=0,
        )
    )
    chunks.append(
        symbol_instance(
            "Connector:Conn_01x05_Socket",
            "J2",
            "COL0-4",
            J2_AT,
            "Connector_JST:JST_XH_B5B-XH-AM_1x05_P2.50mm_Vertical",
            [str(n) for n in range(1, 6)],
        )
    )

    for row in range(8):
        px, py = pico_pin_xy(row + 2)
        jx, jy = j1_pin_xy(row)
        chunks.append(wire(px, py, jx, py))
        chunks.append(junction(jx, py))
        chunks.append(wire(jx, py, jx, jy))
        chunks.append(junction(jx, jy))

    for col in range(5):
        px, py = pico_pin_xy(col + 10)
        jx, jy = j2_pin_xy(col)
        chunks.append(wire(px, py, jx, py))
        chunks.append(junction(jx, py))
        chunks.append(wire(jx, py, jx, jy))
        chunks.append(junction(jx, jy))

    chunks.append(
        text_note(
            25.4,
            175.0,
            [
                "J1 (8p JST ZH): ROW0-7 to button panel row harness",
                "J2 (5p JST XH): COL0-4 to button panel column harness",
                "Common GND between Pico, J1 shell, and J2 return",
            ],
        )
    )

    for pin in (1, 2, 20, 21, 22, 24, 25, 26, 27, 29, 30, 31, 32, 34, 37):
        x, y = pico_pin_world(pin)
        chunks.append(junction(x, y))
        chunks.append(no_connect(x, y))

    body = "".join(chunks)
    lib_symbols = load_embedded_lib_symbols().replace("(lib_symbols", "\t(lib_symbols", 1)
    sch = f"""(kicad_sch
\t(version 20260306)
\t(generator "generate_schematic.py")
\t(generator_version "1.0")
\t(uuid "{ROOT_UUID}")
\t(paper "A4")
{lib_symbols}
{body}
\t(sheet_instances
\t\t(path "/"
\t\t\t(page "1")
\t\t)
\t)
)
"""

    out = Path(__file__).with_name(f"{PROJECT}.kicad_sch")
    out.write_text(sch, encoding="utf-8")
    print(f"wrote {out}")


if __name__ == "__main__":
    main()
