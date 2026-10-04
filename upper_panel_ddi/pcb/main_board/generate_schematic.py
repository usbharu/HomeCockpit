#!/usr/bin/env python3
"""Generate upper_panel_ddi_main_board.kicad_sch from firmware pin assignments."""

from __future__ import annotations

import re
import uuid
from pathlib import Path

ROOT_UUID = "a1b2c3d4-e5f6-7890-abcd-ef1234567890"
PROJECT = "upper_panel_ddi_main_board"
_UID_NS = uuid.UUID(ROOT_UUID)
_uid_seq = 0

PICO_AT = (63.5, 111.76)
ALL_PICO_PINS = [str(n) for n in range(1, 41)]

# Eight button_panel modules, each with JST 1x08 (same pinout as button_panel J1).
PANEL_COUNT = 8
COL_COUNT = 5
PANEL_PITCH = 35.56
PANEL_ORIGIN = (254.0, 60.96)
PANEL_FOOTPRINT = "Connector_JST:JST_ZH_B8B-ZR_1x08_P1.50mm_Vertical"

# GPIOs face the connectors. ROW elbows preserve their order without crossing
# each other; COL feeds pass below the connectors before reaching the trunks.
ROW_ELBOW_X = (109.22, 111.76, 129.54, 127.0, 124.46, 121.92, 119.38, 116.84)
COL_FEED_X = (104.14, 101.6, 99.06, 96.52, 93.98)
COL_TRUNK_X = (203.2, 208.28, 213.36, 218.44, 223.52)
COL_FEED_Y = (335.28, 337.82, 340.36, 342.9, 345.44)

# Pin 2 = panel LED anode (local); pin 8 = reserved — leave unconnected on main harness.
PANEL_NC_PINS = (2, 8)
# Pins 3..7 = COL0..4 (common column bus across all panel connectors).
PANEL_COL_PIN_START = 3

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


def conn08_socket_pin_tip(at: tuple[float, float], pin_number: int) -> tuple[float, float]:
    """Wire anchor for the embedded Conn_01x08_Socket symbol."""
    px, py = at
    local_y = 7.62 - (pin_number - 1) * 2.54
    return (snap_mm(px - 5.08), snap_mm(py - local_y))


def panel_connector_at(index: int) -> tuple[float, float]:
    ox, oy = PANEL_ORIGIN
    return (ox, snap_mm(oy + index * PANEL_PITCH))


def uid(name: str = "obj") -> str:
    global _uid_seq
    _uid_seq += 1
    return str(uuid.uuid5(_UID_NS, f"{name}-{_uid_seq}"))


def snap_mm(value: float) -> float:
    return round(value / 0.254) * 0.254


def pico_pin_world(pin_number: int) -> tuple[float, float]:
    lx, ly, _angle, _length, _ = PICO_PIN_LOCAL[pin_number]
    px, py = PICO_AT
    # A1 is mirrored about Y so its GPIO0..15 pins face right.
    return (snap_mm(px - lx), snap_mm(py - ly))


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


def wire_path(*points: tuple[float, float]) -> list[str]:
    """Emit separate orthogonal segments; crossings do not imply junctions."""
    return [wire(*start, *end) for start, end in zip(points, points[1:]) if start != end]


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
    fields_at: tuple[float, float] | None = None,
    fields_justify: str = "left",
) -> str:
    pin_lines = "\n".join(f'\t\t(pin "{n}"\n\t\t\t(uuid "{uid()}")\n\t\t)' for n in pin_numbers)
    mirror = "\n\t\t(mirror y)" if mirror_y else ""
    fx, fy = fields_at if fields_at is not None else (at[0] + 5.08, at[1] - 7.62)
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
\t\t\t(at {fx} {fy} 0)
\t\t\t(effects
\t\t\t\t(font
\t\t\t\t\t(size 1.27 1.27)
\t\t\t\t)
\t\t\t\t(justify {fields_justify})
\t\t\t)
\t\t)
\t\t(property "Value" "{value}"
\t\t\t(at {fx} {fy + 2.54} 0)
\t\t\t(effects
\t\t\t\t(font
\t\t\t\t\t(size 1.27 1.27)
\t\t\t\t)
\t\t\t\t(justify {fields_justify})
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
    value_y = snap_mm(at[1] + 5.08 if value == "GND" else at[1] - 3.81)
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
\t\t\t(at {at[0]} {value_y} 0)
\t\t\t(effects
\t\t\t\t(font
\t\t\t\t\t(size 1.27 1.27)
\t\t\t\t)
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


def load_embedded_lib_symbols() -> str:
    board_dir = Path(__file__).resolve().parent
    blocks = [
        (board_dir / "embedded_Conn_01x08_Socket.txt").read_text(encoding="utf-8"),
        (board_dir / "embedded_kicad10_RaspberryPi_Pico.txt").read_text(encoding="utf-8"),
        (board_dir / "embedded_kicad10_power_GND.txt").read_text(encoding="utf-8"),
        (board_dir / "embedded_kicad10_power_plus5V.txt").read_text(encoding="utf-8"),
        (board_dir / "embedded_kicad10_power_plus3V3.txt").read_text(encoding="utf-8"),
    ]
    body = "".join(f"\t\t{b.strip()}\n" for b in blocks)
    return f"(lib_symbols\n{body}\t)\n"


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
    global _uid_seq
    _uid_seq = 0
    chunks: list[str] = []

    chunks.append(
        text_note(
            20.32,
            15.24,
            [
                "Upper Panel DDI main board (perfboard) — firmware: upper_panel_ddi",
                "Matrix: 8x5 (GP2-9 rows out, GP10-14 cols in, pulldown in FW)",
                "IMCP: USB CDC on Pico Micro-USB (115200 logical); GP0/GP1 UART NC",
                "Power: USB VBUS; max_power 100 mA in firmware USB descriptor",
                "Eight J1..J8 (1x08 each): Pin1 ROW0-7 -> GP2-9 | Pin3-7 COL0-4 -> GP10-14",
                "Pin2 (panel LED) and Pin8 NC on main",
            ],
        )
    )
    chunks.append(
        text_note(
            20.32,
            363.22,
            [
                "Panel JST 1x08 pinout (same as button_panel J1):",
                "  1=ROWn (this panel)  2=NC (LED on panel)  3..7=COL0..4 (bus)  8=NC",
                "SWD: wire to Pico module debug pads (SWDIO/SWCLK/GND); not on symbol.",
                "Crossings without a junction dot are not connected.",
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
            mirror_y=True,
            fields_at=(PICO_AT[0] - 17.78, 165.1),
        )
    )

    chunks.extend(wire_path(gnd_net, (gnd_net[0], 152.4)))
    chunks.append(power_symbol("#PWR01", "power:GND", (gnd_net[0], 152.4)))
    chunks.extend(wire_path(vbus_net, (vbus_net[0], 66.04), (vbus_net[0], 63.5)))
    chunks.extend(wire_path(vsys_net, (vsys_net[0], 66.04), (vbus_net[0], 66.04)))
    chunks.append(junction(vbus_net[0], 66.04))
    chunks.append(power_symbol("#PWR02", "power:+5V", (vbus_net[0], 63.5)))
    # Route ADC_VREF around the outside of A1, away from the unused GPIO pins.
    chunks.extend(wire_path(mcu_3v3, (mcu_3v3[0], 55.88)))
    chunks.extend(wire_path((mcu_3v3[0], 55.88), (27.94, 55.88), (27.94, adc_ref[1]), adc_ref))
    chunks.append(power_symbol("#PWR03", "power:+3V3", (mcu_3v3[0], 55.88)))
    agnd = pico_pin_world(33)
    chunks.append(no_connect(agnd[0], agnd[1]))

    for panel in range(PANEL_COUNT):
        at = panel_connector_at(panel)
        chunks.append(
            symbol_instance(
                "Connector:Conn_01x08_Socket",
                f"J{panel + 1}",
                f"ROW{panel}",
                at,
                PANEL_FOOTPRINT,
                [str(n) for n in range(1, 9)],
            )
        )
        row_tip = conn08_socket_pin_tip(at, 1)
        row_gpio = pico_pin_xy(panel + 2)
        elbow = ROW_ELBOW_X[panel]
        chunks.extend(wire_path(row_gpio, (elbow, row_gpio[1]), (elbow, row_tip[1]), row_tip))
        # One local label per net names a physically continuous wire.
        chunks.append(net_label(f"ROW{panel}", row_tip[0] - 5.08, row_tip[1]))
        for pin in PANEL_NC_PINS:
            nc = conn08_socket_pin_tip(at, pin)
            chunks.append(no_connect(nc[0], nc[1]))
        for col in range(COL_COUNT):
            pin_number = PANEL_COL_PIN_START + col
            tip = conn08_socket_pin_tip(at, pin_number)
            chunks.extend(wire_path((COL_TRUNK_X[col], tip[1]), tip))
            if panel > 0:
                chunks.append(junction(COL_TRUNK_X[col], tip[1]))

    for col in range(COL_COUNT):
        gpio = pico_pin_xy(col + 10)
        feed_x, feed_y, trunk_x = COL_FEED_X[col], COL_FEED_Y[col], COL_TRUNK_X[col]
        chunks.extend(wire_path(gpio, (feed_x, gpio[1]), (feed_x, feed_y), (trunk_x, feed_y)))
        chunks.append(net_label(f"COL{col}", feed_x, gpio[1]))
        # Split the common vertical wire at every tap. Only same-net taps have
        # junction dots; perpendicular ROW/COL and COL/COL crossings do not.
        tap_ys = [
            conn08_socket_pin_tip(panel_connector_at(panel), PANEL_COL_PIN_START + col)[1]
            for panel in range(PANEL_COUNT)
        ]
        chunks.extend(wire_path(*[(trunk_x, y) for y in [*tap_ys, feed_y]]))

    for pin in (1, 2, 20, 21, 22, 24, 25, 26, 27, 29, 30, 31, 32, 34, 37):
        x, y = pico_pin_world(pin)
        chunks.append(no_connect(x, y))

    body = "".join(chunks)
    lib_symbols = load_embedded_lib_symbols().replace("(lib_symbols", "\t(lib_symbols", 1)
    sch = f"""(kicad_sch
\t(version 20260306)
\t(generator "generate_schematic.py")
\t(generator_version "1.0")
\t(uuid "{ROOT_UUID}")
\t(paper "A3" portrait)
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
