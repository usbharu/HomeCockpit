import importlib.util
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch


spec = importlib.util.spec_from_file_location(
    "previews", Path(__file__).resolve().parents[1] / "render-kicad-previews.py",
)
previews = importlib.util.module_from_spec(spec)
spec.loader.exec_module(previews)


class SelectionTests(unittest.TestCase):
    def setUp(self):
        self.board = "pcb/button/button.kicad_pcb"
        self.sch = "pcb/button/button.kicad_sch"
        self.main = "pcb/main/main.kicad_sch"
        self.files = {self.board, self.sch}

    def test_new_schematic_only_project_without_ci_registration(self):
        entries = previews.select_designs({self.main}, self.files, self.files | {self.main})
        self.assertEqual([(e["source"], e["status"]) for e in entries], [(self.main, "added")])
        self.assertFalse(entries[0]["before"])
        self.assertTrue(entries[0]["after"])

    def test_project_configuration_renders_both_board_and_schematic(self):
        entries = previews.select_designs({"pcb/button/button.kicad_pro"}, self.files, self.files)
        self.assertEqual({e["source"] for e in entries}, self.files)
        self.assertEqual({e["status"] for e in entries}, {"related"})

    def test_shared_library_renders_every_project(self):
        for changed in ["HomeCockpit.kicad_sym", "Library.pretty/switch.kicad_mod", "pcb/main/sym-lib-table"]:
            entries = previews.select_designs({changed}, self.files, self.files | {self.main})
            self.assertEqual({e["source"] for e in entries}, self.files | {self.main})

    def test_deleted_and_renamed_designs_keep_before_image(self):
        renamed = "pcb/new name/button.kicad_pcb"
        entries = previews.select_designs({self.board, renamed}, self.files, {self.sch, renamed})
        by_path = {e["source"]: e for e in entries}
        self.assertEqual(by_path[self.board]["status"], "deleted")
        self.assertTrue(by_path[self.board]["before"])
        self.assertFalse(by_path[self.board]["after"])
        self.assertEqual(by_path[renamed]["status"], "added")

    def test_child_sheet_renders_parent_schematic(self):
        child = "pcb/button/sheets/connectors.kicad_sch"
        entries = previews.select_designs({child}, self.files | {child}, self.files | {child})
        self.assertEqual({e["source"] for e in entries}, self.files | {child})

    def test_other_projects_and_non_design_files_do_not_trigger_rendering(self):
        self.assertEqual(previews.select_designs({"manager/src/app.tsx"}, self.files, self.files), [])
        self.assertEqual(previews.select_designs({"pcb/button/button.kicad_prl"}, self.files, self.files), [])

    def test_git_plan_uses_merge_base_and_handles_space_in_new_path(self):
        with tempfile.TemporaryDirectory() as temporary:
            repo = Path(temporary)
            def git(*args):
                return subprocess.check_output([
                    "git", "-c", "core.hooksPath=/dev/null", "-c", "core.fsmonitor=false",
                    "-C", str(repo), *args,
                ])
            git("init", "-q")
            git("config", "user.name", "Test")
            git("config", "user.email", "test@example.com")
            (repo / "README.md").write_text("base")
            git("add", ".")
            git("commit", "-qm", "base")
            base = git("rev-parse", "HEAD").decode().strip()
            git("checkout", "-qb", "pr")
            name = "new board/new.kicad_sch"
            (repo / "new board").mkdir()
            (repo / name).write_text("new schematic")
            git("add", ".")
            git("commit", "-qm", "add schematic")
            head = git("rev-parse", "HEAD").decode().strip()
            git("checkout", "-q", "--detach", base)
            (repo / "unrelated.kicad_pcb").write_text("unrelated master change")
            git("add", ".")
            git("commit", "-qm", "master change")
            master = git("rev-parse", "HEAD").decode().strip()
            with patch.object(previews, "git", git):
                plan = previews.plan(master, head)
            self.assertEqual(plan["base"], base)
            self.assertEqual([(e["source"], e["status"]) for e in plan["entries"]], [(name, "added")])


class RenderTests(unittest.TestCase):
    def test_failed_export_keeps_successful_images_and_records_failure(self):
        files = {"main.kicad_pcb", "main.kicad_sch"}
        manifest = {"base": "base", "head": "head", "entries": previews.select_designs(files, files, files)}
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary)
            previews.save(manifest, output)
            def render(entry, side, workspace, output):
                if side == "before" and entry["source"].endswith(".kicad_pcb"):
                    raise subprocess.CalledProcessError(3, ["kicad-cli"])
                entry["images"][side]["view"] = f"{side}.png"
            with patch.object(sys, "argv", ["previews", "--output", str(output)]), \
                    patch.object(previews, "snapshot"), patch.object(previews, "render", side_effect=render):
                with self.assertRaises(SystemExit) as result:
                    previews.main()
            self.assertEqual(result.exception.code, 1)
            data = previews.json.loads((output / "manifest.json").read_text())
        self.assertIn("before", data["entries"][0]["errors"])
        self.assertEqual(data["entries"][0]["images"]["after"]["view"], "after.png")
        self.assertEqual(data["entries"][1]["images"]["before"]["view"], "before.png")

    def test_schematic_export_preserves_all_pages(self):
        entry = previews.select_designs({"main.kicad_sch"}, set(), {"main.kicad_sch"})[0]
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary)
            def export(args, workspace, output):
                target = output / Path(args[args.index("--output") + 1]).name
                for page in ["main", "main-connectors"]:
                    (target / f"{page}.svg").write_text("<svg/>")
            with patch.object(previews, "run_kicad", side_effect=export), patch.object(previews.subprocess, "run"):
                previews.render(entry, "after", output, output)
        self.assertEqual(set(entry["images"]["after"]), {"sheet: main", "sheet: main-connectors"})

    def test_board_has_separate_front_and_mirrored_back(self):
        entry = previews.select_designs({"main.kicad_pcb"}, set(), {"main.kicad_pcb"})[0]
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary)
            with patch.object(previews, "run_kicad") as export, patch.object(previews.subprocess, "run"):
                previews.render(entry, "after", output, output)
        self.assertEqual(set(entry["images"]["after"]), {"front", "back"})
        self.assertNotIn("--mirror", export.call_args_list[0].args[0])
        self.assertIn("--mirror", export.call_args_list[1].args[0])


if __name__ == "__main__":
    unittest.main()
