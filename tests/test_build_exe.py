from __future__ import annotations

import importlib.util
from pathlib import Path
from types import SimpleNamespace
import unittest


ROOT = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location(
    "build_exe", ROOT / "scripts" / "build_exe.py"
)
build_exe = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(build_exe)


class BuildSpecTests(unittest.TestCase):
    def execute_spec(self, onefile: bool, gui_binaries, service_binaries):
        """Exercise the generated spec without building or launching the app."""
        binaries_by_script = {
            "run.py": gui_binaries,
            "run_service.py": service_binaries,
        }

        def analysis(scripts, **kwargs):
            return SimpleNamespace(
                binaries=list(binaries_by_script[scripts[0]]),
                datas=list(kwargs["datas"]),
                pure=[],
                scripts=scripts,
            )

        def target(*args, **kwargs):
            return SimpleNamespace(args=args, kwargs=kwargs)

        namespace = {
            "Analysis": analysis,
            "PYZ": target,
            "EXE": target,
            "COLLECT": target,
        }
        content = build_exe.build_spec_content(onefile, "test-output")
        exec(compile(content, "sysu_netauth.spec", "exec"), namespace)
        return namespace

    def test_system_icu_is_excluded_from_both_executables_and_layouts(self):
        # Either Analysis can resolve a same-named third-party ICU from PATH.
        # A versioned ICU library and unrelated DLLs must still be bundled.
        gui_keep = [
            ("PySide6/Qt6Core.dll", "qt/Qt6Core.dll", "BINARY"),
            ("icuuc78.dll", "icu/icuuc78.dll", "BINARY"),
        ]
        service_keep = [("python313.dll", "python/python313.dll", "BINARY")]
        gui_binaries = gui_keep + [
            ("icuuc.dll", "conda/Library/bin/icuuc.dll", "BINARY"),
            ("vendor\\ICuUc.DlL", "other/icuuc.dll", "BINARY"),
        ]
        service_binaries = service_keep + [
            ("ICUUC.DLL", "path/icuuc.dll", "BINARY"),
            ("vendor/icuuc.dll", "other/icuuc.dll", "BINARY"),
        ]

        for onefile in (False, True):
            with self.subTest(onefile=onefile):
                namespace = self.execute_spec(onefile, gui_binaries, service_binaries)
                self.assertEqual(namespace["a"].binaries, gui_keep)
                self.assertEqual(namespace["service_a"].binaries, service_keep)
                if onefile:
                    self.assertEqual(namespace["exe"].args[2], gui_keep)
                    self.assertEqual(namespace["service_exe"].args[2], service_keep)
                    self.assertNotIn("coll", namespace)
                else:
                    self.assertEqual(namespace["coll"].args[2], gui_keep)
                    self.assertEqual(namespace["coll"].args[4], service_keep)

    def test_build_without_system_icu_preserves_existing_binaries(self):
        binaries = [("PySide6/Qt6Core.dll", "qt/Qt6Core.dll", "BINARY")]
        for onefile in (False, True):
            with self.subTest(onefile=onefile):
                namespace = self.execute_spec(onefile, binaries, [])
                self.assertEqual(namespace["a"].binaries, binaries)
                self.assertEqual(namespace["service_a"].binaries, [])


if __name__ == "__main__":
    unittest.main()
