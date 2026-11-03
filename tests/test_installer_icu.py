from __future__ import annotations

import re
import shutil
import tempfile
import unittest
from pathlib import Path

from scripts import build


ROOT = Path(__file__).resolve().parent.parent


def section_entries(script: str, section: str) -> list[dict[str, str]]:
    """Read the simple parameter entries used by these installer sections."""
    contents = re.search(
        rf"^\[{section}\]\s*\n(.*?)(?=^\[|\Z)", script, re.MULTILINE | re.DOTALL
    )
    if contents is None:
        return []
    entries = []
    for line in contents.group(1).splitlines():
        if not line.strip() or line.lstrip().startswith(";"):
            continue
        entries.append(
            {
                name.strip(): value.strip().strip('"')
                for name, value in (
                    parameter.split(":", 1) for parameter in line.split(";")
                )
            }
        )
    return entries


class InstallerIcuTests(unittest.TestCase):
    def setUp(self) -> None:
        temporary = tempfile.TemporaryDirectory(prefix="netauth installer ")
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name)
        config = build.BuildConfig()
        config.project_root = root
        config.script_dir = root / "scripts"
        config.app_dir = root / "dist" / "sysu_netauth"
        config.script_dir.mkdir()
        (root / "updates").mkdir()
        config.app_dir.mkdir(parents=True)
        (config.app_dir / build.APP_EXE_NAME).write_bytes(b"GUI fixture")
        shutil.copyfile(
            ROOT / "scripts" / "setup.template.iss",
            config.script_dir / "setup.template.iss",
        )
        shutil.copyfile(
            ROOT / "updates" / "release.json", root / "updates" / "release.json"
        )
        self.script = build.generate_iss_file(config).read_text(encoding="utf-8")

    def test_upgrade_deletes_only_the_conflicting_private_dll(self) -> None:
        self.assertEqual(
            section_entries(self.script, "InstallDelete"),
            [{"Type": "files", "Name": r"{app}\_internal\icuuc.dll"}],
        )

    def test_existing_build_cannot_reintroduce_icu_while_other_payload_is_kept(
        self,
    ) -> None:
        files = section_entries(self.script, "Files")
        payload = next(entry for entry in files if entry["Source"].endswith(r"\*"))
        # A leading backslash anchors Inno's exclusion at the payload root.
        self.assertEqual(
            payload["Excludes"].split(","),
            ["{#MyAppExeName}", r"\_internal\icuuc.dll"],
        )
        self.assertEqual(payload["DestDir"], "{app}")
        self.assertIn("recursesubdirs", payload["Flags"].split())
        self.assertIn("createallsubdirs", payload["Flags"].split())
        self.assertTrue(
            any(
                entry["Source"].endswith(r"\{#MyAppExeName}")
                and entry["DestDir"] == "{app}"
                for entry in files
            )
        )


if __name__ == "__main__":
    unittest.main()
