#!/usr/bin/env python3
"""Render changed KiCad projects at the PR merge base and head (no ERC/DRC)."""

import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import subprocess
import tarfile
import tempfile


KICAD_IMAGE = "ghcr.io/kicad/kicad:10.0.5"
KICAD10_SYMBOL_DIR = "/usr/share/kicad/symbols"
DESIGN_SUFFIXES = {".kicad_pcb", ".kicad_sch"}
PROJECT_SUFFIXES = DESIGN_SUFFIXES | {".kicad_pro", ".kicad_dru"}
LIBRARY_SUFFIXES = {".kicad_sym", ".kicad_mod"}


def git(*args):
    return subprocess.check_output(["git", *args])


def paths(data):
    return {item.decode("utf-8") for item in data.split(b"\0") if item}


def select_designs(changed, before, after):
    """Include sibling boards/schematics and hierarchical sheets, in any project."""
    designs = {p for p in before | after if PurePosixPath(p).suffix in DESIGN_SUFFIXES}
    # Library and library-table edits may affect projects anywhere in the repo.
    shared = any(
        PurePosixPath(p).suffix in LIBRARY_SUFFIXES
        or PurePosixPath(p).name in {"sym-lib-table", "fp-lib-table"}
        for p in changed
    )
    directories = {
        PurePosixPath(p).parent
        for p in changed
        if PurePosixPath(p).suffix in PROJECT_SUFFIXES
    }
    # A nested sheet can change a root schematic in a parent directory.
    schematic_roots = {
        PurePosixPath(p).parent
        for p in designs
        if PurePosixPath(p).suffix == ".kicad_sch"
        and any(
            PurePosixPath(c).suffix == ".kicad_sch"
            and PurePosixPath(c).is_relative_to(PurePosixPath(p).parent)
            for c in changed
        )
    }
    directories |= schematic_roots
    selected = sorted(
        p for p in designs
        if shared or any(PurePosixPath(p).is_relative_to(d) for d in directories)
    )
    return [
        {
            "source": p,
            "status": (
                "added" if p not in before else "deleted" if p not in after
                else "modified" if p in changed else "related"
            ),
            "before": p in before,
            "after": p in after,
            "images": {"before": {}, "after": {}},
            "errors": {},
        }
        for p in selected
    ]


def plan(base, head):
    # Use the PR merge base, so unrelated changes on master are not shown as edits.
    base = git("merge-base", base, head).decode().strip()
    before = paths(git("ls-tree", "-r", "--name-only", "-z", base))
    after = paths(git("ls-tree", "-r", "--name-only", "-z", head))
    # Treat renames as a deletion and an addition so both versions remain visible.
    changed = paths(git("diff", "--no-renames", "--name-only", "-z", base, head))
    return {"base": base, "head": head, "entries": select_designs(changed, before, after)}


def snapshot(revision, destination):
    destination.mkdir()
    with tempfile.TemporaryFile() as archive:
        subprocess.run(["git", "archive", revision], stdout=archive, check=True)
        archive.seek(0)
        with tarfile.open(fileobj=archive) as contents:
            contents.extractall(destination, filter="data")


def run_kicad(arguments, workspace, output, project_dir=None):
    """Run kicad-cli in Docker; project_dir enables sym-lib-table ${KIPRJMOD} resolution."""
    workdir = "/workspace"
    env = [
        "-e", "HOME=/tmp/kicad-home",
        "-e", f"KICAD10_SYMBOL_DIR={KICAD10_SYMBOL_DIR}",
    ]
    if project_dir:
        env.extend(["-e", f"KIPRJMOD=/workspace/{project_dir}"])
        workdir = f"/workspace/{project_dir}"
    subprocess.run(
        [
            "docker", "run", "--rm", "--network", "none",
            *env,
            "-v", f"{workspace}:/workspace:ro",
            "-v", f"{output}:/renders",
            "-w", workdir, KICAD_IMAGE, "kicad-cli", *arguments,
        ],
        check=True,
        timeout=180,
    )


def render(entry, side, workspace, output):
    source = entry["source"]
    project_dir = str(PurePosixPath(source).parent)
    # Full-path hash prevents collisions between identically named project files.
    identifier = hashlib.sha256(source.encode()).hexdigest()[:20]
    target = output / f"{identifier}-{side}"
    target.mkdir()
    target.chmod(0o777)
    svg_paths = {}
    if PurePosixPath(source).suffix == ".kicad_pcb":
        for view, layers in (
            ("front", "F.Cu,F.Mask,F.Silkscreen,Edge.Cuts"),
            ("back", "B.Cu,B.Mask,B.Silkscreen,Edge.Cuts"),
        ):
            name = f"{view}.svg"
            run_kicad(
                [
                    "pcb", "export", "svg", "--page-size-mode", "2",
                    "--exclude-drawing-sheet", "--mode-single", "--layers", layers,
                    *(["--mirror"] if view == "back" else []),
                    "--output", f"/renders/{target.name}/{name}", f"/workspace/{source}",
                ],
                workspace, output, project_dir=project_dir,
            )
            svg_paths[view] = target / name
    else:
        sch_name = PurePosixPath(source).name
        run_kicad(
            [
                "sch", "export", "svg", "--output", f"/renders/{target.name}/",
                sch_name,
            ],
            workspace, output, project_dir=project_dir,
        )
        # Keep all hierarchical pages, matched by name between revisions.
        svg_paths = {f"sheet: {p.stem}": p for p in sorted(target.glob("*.svg"))}
    if not svg_paths:
        raise RuntimeError("KiCad did not export any SVG pages")
    for label, svg in svg_paths.items():
        name = f"{identifier}-{side}-{hashlib.sha256(label.encode()).hexdigest()[:12]}.png"
        subprocess.run(
            [
                "rsvg-convert", "--width", "1800", "--keep-aspect-ratio",
                "--background-color", "#1b1e28" if source.endswith(".kicad_pcb") else "white",
                "--output", str(output / name), str(svg),
            ],
            check=True,
            timeout=60,
        )
        entry["images"][side][label] = name


def save(manifest, output):
    (output / "manifest.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8",
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base")
    parser.add_argument("--head")
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--plan-only", action="store_true")
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    output.chmod(0o777)
    if args.plan_only:
        if not args.base or not args.head:
            parser.error("--plan-only requires --base and --head")
        save(plan(args.base, args.head), output)
        return

    manifest = json.loads((output / "manifest.json").read_text(encoding="utf-8"))
    failed = False
    with tempfile.TemporaryDirectory(prefix="kicad-preview-") as temporary:
        for side, revision in (("before", manifest["base"]), ("after", manifest["head"])):
            entries = [entry for entry in manifest["entries"] if entry[side]]
            if not entries:
                continue
            workspace = Path(temporary) / side
            snapshot(revision, workspace)
            for entry in entries:
                try:
                    render(entry, side, workspace, output)
                except (subprocess.SubprocessError, OSError, RuntimeError) as error:
                    # Publish the successful images even if one version fails to export.
                    entry["errors"][side] = str(error)
                    failed = True
                save(manifest, output)
    if failed:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
