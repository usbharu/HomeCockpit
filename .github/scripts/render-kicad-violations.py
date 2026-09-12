#!/usr/bin/env python3
"""Render KiCad ERC/DRC locations on top of a KiCad SVG export.

KiCad's text reports contain the display-formatted design coordinates while its
SVG plotter emits millimetre-based page coordinates with the Y axis flipped.
This script uses those coordinates for the overlay and keeps the SVG as a
vector image, so the CI job does not need ImageMagick or another graphics
package.
"""

from __future__ import annotations

import argparse
import html
import json
import re
import sys
from pathlib import Path
from typing import Any


NUMBER = r"[-+]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][-+]?\d+)?"
COORDINATE_RE = re.compile(
    rf"\((?:start|end|center|mid|at|xy)\s+({NUMBER})\s+({NUMBER})"
)
REPORT_POSITION_RE = re.compile(
    rf"@\(\s*({NUMBER})\s*([A-Za-z]+)\s*,\s*({NUMBER})\s*([A-Za-z]+)\s*\)"
)
SVG_ROOT_RE = re.compile(r"<svg\b(?P<attributes>[^>]*)>", re.IGNORECASE | re.DOTALL)
SVG_ATTRIBUTE_RE = re.compile(
    r"\b(?P<name>[A-Za-z_:][\w:.-]*)\s*=\s*(?P<quote>[\"'])(?P<value>.*?)(?P=quote)",
    re.DOTALL,
)

UNIT_SCALE_TO_MM = {
    "mm": 1.0,
    "cm": 10.0,
    "in": 25.4,
    "pt": 25.4 / 72.0,
    "pc": 25.4 / 6.0,
    "px": 25.4 / 96.0,
}


def format_number(value: float) -> str:
    if abs(value) < 0.0000001:
        value = 0.0
    return f"{value:.4f}"


def read_attribute(attributes: str, name: str) -> str | None:
    for match in SVG_ATTRIBUTE_RE.finditer(attributes):
        if match.group("name").lower() == name.lower():
            return match.group("value")
    return None


def parse_length_mm(value: str | None) -> float | None:
    if value is None:
        return None
    match = re.fullmatch(rf"\s*({NUMBER})\s*([A-Za-z%]*)\s*", value)
    if match is None:
        return None
    unit = match.group(2).lower()
    scale = UNIT_SCALE_TO_MM.get(unit)
    if scale is None:
        return None
    return float(match.group(1)) * scale


def replace_root_attribute(text: str, name: str, value: str) -> str:
    root = SVG_ROOT_RE.search(text)
    if root is None:
        return text

    attributes = root.group("attributes")
    pattern = re.compile(
        rf"(\b{re.escape(name)}\s*=\s*)([\"'])(.*?)(\2)",
        re.IGNORECASE | re.DOTALL,
    )
    replaced, count = pattern.subn(
        lambda match: f"{match.group(1)}{match.group(2)}{value}{match.group(2)}",
        attributes,
        count=1,
    )
    if count == 0:
        replaced = f'{attributes} {name}="{value}"'

    start = root.start("attributes")
    end = root.end("attributes")
    return text[:start] + replaced + text[end:]


def parse_svg(text: str) -> dict[str, float]:
    root = SVG_ROOT_RE.search(text)
    if root is None:
        raise ValueError("SVG root element was not found")

    viewbox = read_attribute(root.group("attributes"), "viewBox")
    if viewbox is None:
        raise ValueError("SVG viewBox attribute was not found")

    values = [float(value) for value in re.split(r"[\s,]+", viewbox.strip()) if value]
    if len(values) != 4 or values[2] <= 0 or values[3] <= 0:
        raise ValueError(f"Invalid SVG viewBox: {viewbox}")

    view_x, view_y, view_width, view_height = values
    physical_width = parse_length_mm(read_attribute(root.group("attributes"), "width"))
    physical_height = parse_length_mm(read_attribute(root.group("attributes"), "height"))

    scale_x = view_width / physical_width if physical_width and physical_width > 0 else 1.0
    scale_y = view_height / physical_height if physical_height and physical_height > 0 else 1.0

    return {
        "view_x": view_x,
        "view_y": view_y,
        "view_width": view_width,
        "view_height": view_height,
        "coordinate_view_x": view_x,
        "coordinate_view_y": view_y,
        "physical_width": physical_width or view_width,
        "physical_height": physical_height or view_height,
        "page_width": physical_width or view_width,
        "page_height": physical_height or view_height,
        "scale_x": scale_x,
        "scale_y": scale_y,
    }


def find_balanced_block(text: str, start: int) -> str:
    depth = 0
    quoted = False
    escaped = False

    for index in range(start, len(text)):
        character = text[index]
        if quoted:
            if escaped:
                escaped = False
            elif character == "\\":
                escaped = True
            elif character == '"':
                quoted = False
            continue

        if character == '"':
            quoted = True
        elif character == "(":
            depth += 1
        elif character == ")":
            depth -= 1
            if depth == 0:
                return text[start : index + 1]

    return text[start:]


def edge_cuts_bbox(board_path: Path) -> tuple[float, float, float, float] | None:
    text = board_path.read_text(encoding="utf-8")
    block_re = re.compile(r"\(gr_(?:line|arc|circle|curve|rect|poly)\b")
    points: list[tuple[float, float]] = []

    for match in block_re.finditer(text):
        block = find_balanced_block(text, match.start())
        if re.search(r'\(layer\s+"Edge\.Cuts"\)', block) is None:
            continue
        points.extend((float(x), float(y)) for x, y in COORDINATE_RE.findall(block))

    if not points:
        return None

    xs = [point[0] for point in points]
    ys = [point[1] for point in points]
    return min(xs), min(ys), max(xs), max(ys)


def crop_to_edge_cuts(
    svg_text: str, svg: dict[str, float], board_path: Path | None
) -> tuple[str, dict[str, float]]:
    if board_path is None or not board_path.is_file():
        return svg_text, svg

    bbox = edge_cuts_bbox(board_path)
    if bbox is None:
        return svg_text, svg

    min_x, min_y, max_x, max_y = bbox
    margin = 3.0
    min_x -= margin
    min_y -= margin
    max_x += margin
    max_y += margin

    crop_x = svg["view_x"] + min_x * svg["scale_x"]
    crop_y = svg["view_y"] + (
        svg["page_height"] - max_y
    ) * svg["scale_y"]
    crop_width = (max_x - min_x) * svg["scale_x"]
    crop_height = (max_y - min_y) * svg["scale_y"]
    crop_physical_width = max_x - min_x
    crop_physical_height = max_y - min_y

    cropped = replace_root_attribute(
        svg_text,
        "viewBox",
        " ".join(
            format_number(value)
            for value in (crop_x, crop_y, crop_width, crop_height)
        ),
    )
    cropped = replace_root_attribute(
        cropped, "width", f"{format_number(crop_physical_width)}mm"
    )
    cropped = replace_root_attribute(
        cropped, "height", f"{format_number(crop_physical_height)}mm"
    )

    cropped_svg = dict(svg)
    cropped_svg.update(
        {
            "view_x": crop_x,
            "view_y": crop_y,
            "view_width": crop_width,
            "view_height": crop_height,
            "physical_width": crop_physical_width,
            "physical_height": crop_physical_height,
        }
    )
    return cropped, cropped_svg


def coordinate_to_svg(
    x: float, y: float, svg: dict[str, float], coordinate_units: str
) -> tuple[float, float]:
    unit_scale = {"mm": 1.0, "mils": 0.0254, "in": 25.4}.get(coordinate_units, 1.0)
    x_mm = x * unit_scale
    y_mm = y * unit_scale

    # KiCad's SVG plotter uses millimetres and reverses the Y axis so that the
    # result can be displayed in the normal SVG top-to-bottom coordinate space.
    svg_x = svg["coordinate_view_x"] + x_mm * svg["scale_x"]
    svg_y = svg["coordinate_view_y"] + (
        svg["page_height"] - y_mm
    ) * svg["scale_y"]
    return svg_x, svg_y


def collect_json_violations(report: dict[str, Any], kind: str) -> list[dict[str, Any]]:
    raw: list[tuple[str, dict[str, Any]]] = []
    if kind == "pcb":
        for section in ("violations", "unconnected_items", "schematic_parity"):
            raw.extend((section, item) for item in report.get(section, []))
    else:
        for sheet in report.get("sheets", []):
            sheet_path = str(sheet.get("path", "/"))
            raw.extend(
                (f"sheet {sheet_path}", item)
                for item in sheet.get("violations", [])
            )

    records: list[dict[str, Any]] = []
    for section, violation in raw:
        if violation.get("excluded"):
            continue
        severity = str(violation.get("severity", "error")).lower()
        if severity not in {"", "error"}:
            continue

        points: list[tuple[float, float]] = []
        for item in violation.get("items", []):
            position = item.get("pos", {})
            try:
                points.append((float(position["x"]), float(position["y"])))
            except (KeyError, TypeError, ValueError):
                continue

        records.append(
            {
                "section": section,
                "type": str(violation.get("type", "unknown")),
                "description": " ".join(
                    str(violation.get("description", "")).split()
                ),
                "points": points,
            }
        )

    return records


def collect_text_violations(text: str, kind: str) -> list[dict[str, Any]]:
    """Read error positions from KiCad's human-readable report.

    KiCad 9.0.9 writes ERC JSON coordinates with a different scale from its
    text report.  The text report is also useful as a version-independent
    fallback because it is the same report a developer sees in the CLI.
    """

    header_re = re.compile(r"^\s*\[([^\]]+)\]:\s*(.*?)\s*$")
    severity_re = re.compile(r";\s*(error|warning|exclusion)\s*$", re.IGNORECASE)
    current: dict[str, Any] | None = None
    records: list[dict[str, Any]] = []

    def finish() -> None:
        if current is None or str(current.get("severity", "")).lower() != "error":
            return
        records.append(current.copy())

    for line in text.splitlines():
        header = header_re.match(line)
        if header is not None:
            finish()
            violation_type = header.group(1)
            current = {
                "section": (
                    "violations"
                    if kind == "sch"
                    else (
                        "unconnected_items"
                        if violation_type == "unconnected_items"
                        else "violations"
                    )
                ),
                "type": violation_type,
                "description": header.group(2),
                "points": [],
                "severity": None,
            }
            continue

        if current is None:
            continue

        severity = severity_re.search(line)
        if severity is not None:
            current["severity"] = severity.group(1).lower()

        position = REPORT_POSITION_RE.search(line)
        if position is not None:
            x, x_unit, y, y_unit = position.groups()
            if x_unit.lower() == y_unit.lower():
                current["points"].append((float(x), float(y)))

    finish()
    return records


def collect_violations(
    report: dict[str, Any], kind: str, text_report: str | None
) -> list[dict[str, Any]]:
    json_records = collect_json_violations(report, kind)
    if text_report is None:
        return json_records

    text_records = collect_text_violations(text_report, kind)
    if text_records and len(text_records) == len(json_records):
        return text_records
    return json_records


def markdown_cell(value: str) -> str:
    return value.replace("|", "\\|").replace("\n", " ")


def write_summary(
    summary_path: Path,
    title: str,
    kind: str,
    report: dict[str, Any],
    records: list[dict[str, Any]],
) -> None:
    coordinate_units = str(report.get("coordinate_units", "mm"))
    check_name = "DRC" if kind == "pcb" else "ERC"
    lines = [f"## KiCad {check_name}: {title}", ""]

    if records:
        lines.append(
            f"Found **{len(records)} error-level {check_name} violation(s)**. "
            "The annotated SVG uses the same number for all affected items in one violation."
        )
    else:
        lines.append(f"No error-level {check_name} violations were found.")

    lines.extend(
        [
            "",
            "| # | Category | Coordinates | Description |",
            "| ---: | --- | --- | --- |",
        ]
    )
    for index, record in enumerate(records, start=1):
        positions = ", ".join(
            f"({x:g}, {y:g} {coordinate_units})" for x, y in record["points"]
        )
        if not positions:
            positions = "(no coordinate)"
        category = f"{record['section']}: {record['type']}"
        lines.append(
            f"| {index} | {markdown_cell(category)} | {markdown_cell(positions)} | "
            f"{markdown_cell(record['description'])} |"
        )

    summary_path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def make_overlay(
    svg: dict[str, float], title: str, kind: str, records: list[dict[str, Any]], report: dict[str, Any]
) -> str:
    check_name = "DRC" if kind == "pcb" else "ERC"
    units = str(report.get("coordinate_units", "mm"))

    min_dimension = min(svg["view_width"], svg["view_height"])
    radius = max(min_dimension * 0.008, 1.5)
    stroke_width = max(radius * 0.16, 0.25)
    font_size = radius * 0.95
    legend_x = svg["view_x"] + min_dimension * 0.012
    legend_y = svg["view_y"] + min_dimension * 0.012
    legend_width = min(svg["view_width"] * 0.72, max(min_dimension * 0.62, 42.0))
    legend_height = max(min_dimension * 0.075, 12.0)
    title_text = f"KiCad {check_name}: {len(records)} error-level violation(s)"
    subtitle = "Red markers identify affected items"
    if not records:
        title_text = f"KiCad {check_name}: no error-level violations"
        subtitle = "The check passed at error severity"

    parts = [
        '<g id="ci-kicad-violations" pointer-events="none">',
        f'<title>{html.escape(title)} - {html.escape(title_text)}</title>',
        (
            f'<rect x="{format_number(legend_x)}" y="{format_number(legend_y)}" '
            f'width="{format_number(legend_width)}" height="{format_number(legend_height)}" '
            'rx="1.5" fill="#111827" fill-opacity="0.88" />'
        ),
        (
            f'<text x="{format_number(legend_x + radius * 0.55)}" '
            f'y="{format_number(legend_y + radius * 1.25)}" fill="#ffffff" '
            f'font-family="sans-serif" font-size="{format_number(font_size * 0.95)}" '
            'font-weight="bold">'
            f"{html.escape(title_text)}</text>"
        ),
        (
            f'<text x="{format_number(legend_x + radius * 0.55)}" '
            f'y="{format_number(legend_y + radius * 2.45)}" fill="#fecaca" '
            f'font-family="sans-serif" font-size="{format_number(font_size * 0.75)}">'
            f"{html.escape(subtitle)}</text>"
        ),
    ]

    for index, record in enumerate(records, start=1):
        mapped = [
            coordinate_to_svg(x, y, svg, units) for x, y in record["points"]
        ]
        if len(mapped) > 1:
            path = " ".join(
                (
                    ("M" if point_index == 0 else "L")
                    + f" {format_number(x)} {format_number(y)}"
                )
                for point_index, (x, y) in enumerate(mapped)
            )
            parts.append(
                f'<path d="{path}" fill="none" stroke="#f97316" '
                f'stroke-width="{format_number(stroke_width * 0.8)}" '
                'stroke-dasharray="2 1" stroke-opacity="0.85" />'
            )

        description = f"{record['type']}: {record['description']}"
        for point_index, (x, y) in enumerate(mapped, start=1):
            label = str(index) if len(mapped) == 1 else f"{index}.{point_index}"
            parts.extend(
                [
                    "<g>",
                    f"<title>{html.escape(description)}</title>",
                    (
                        f'<circle cx="{format_number(x)}" cy="{format_number(y)}" '
                        f'r="{format_number(radius)}" fill="#dc2626" fill-opacity="0.9" '
                        f'stroke="#ffffff" stroke-width="{format_number(stroke_width)}" />'
                    ),
                    (
                        f'<text x="{format_number(x)}" y="{format_number(y + font_size * 0.35)}" '
                        'text-anchor="middle" fill="#ffffff" font-family="sans-serif" '
                        f'font-size="{format_number(font_size)}" font-weight="bold">'
                        f"{html.escape(label)}</text>"
                    ),
                    "</g>",
                ]
            )

    parts.append("</g>")
    return "\n".join(parts)


def annotate_svg(
    svg_text: str, svg: dict[str, float], overlay: str
) -> str:
    closing_tag = re.search(r"</svg\s*>", svg_text, re.IGNORECASE)
    if closing_tag is None:
        raise ValueError("SVG closing tag was not found")
    return svg_text[: closing_tag.start()] + "\n" + overlay + "\n" + svg_text[closing_tag.start() :]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--kind", choices=("pcb", "sch"), required=True)
    parser.add_argument("--svg", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--summary", type=Path, required=True)
    parser.add_argument("--text-report", type=Path)
    parser.add_argument("--source", type=Path)
    parser.add_argument("--title", required=True)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        svg_text = args.svg.read_text(encoding="utf-8")
        report = json.loads(args.report.read_text(encoding="utf-8"))
        text_report = (
            args.text_report.read_text(encoding="utf-8")
            if args.text_report is not None and args.text_report.is_file()
            else None
        )
        svg = parse_svg(svg_text)
        svg_text, svg = crop_to_edge_cuts(
            svg_text, svg, args.source if args.kind == "pcb" else None
        )
        records = collect_violations(report, args.kind, text_report)
        overlay = make_overlay(svg, args.title, args.kind, records, report)
        annotated = annotate_svg(svg_text, svg, overlay)
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.summary.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(annotated, encoding="utf-8")
        write_summary(args.summary, args.title, args.kind, report, records)
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(f"KiCad visualization failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
