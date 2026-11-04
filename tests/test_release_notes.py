import importlib.util
from pathlib import Path
import unittest

ROOT=Path(__file__).resolve().parent.parent
SPEC=importlib.util.spec_from_file_location('release_notes',ROOT/'scripts/release_notes.py')
notes=importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(notes)

class ReleaseNotesTests(unittest.TestCase):
    def test_exact_release_section_without_other_versions(self):
        source='# Updates\n\n## [1.2.30](https://example.invalid) - date\n\nnewer\n\n## [1.2.3] - date\n\n目标版本\n\n## [1.2.2] - date\n\nolder\n'
        result=notes.extract(source,'1.2.3')
        self.assertTrue(result.startswith('SYSU NetAuth v1.2.3\n\n目标版本\n'))
        self.assertIn('[完整更新日志]',result)
        self.assertNotIn('newer',result)
        self.assertNotIn('older',result)
    def test_missing_empty_duplicate_and_prerelease_sections_block_release(self):
        for source,version in [('# empty\n','1.2.3'),('## [1.2.3]\n','1.2.3'),('## [1.2.3]\na\n## [1.2.3]\nb\n','1.2.3'),('## [1.2.3-beta.1]\nbeta\n','1.2.3-beta.1'),('## [Unreleased]\nx\n## [1.2.3]\nstable\n','1.2.3')]:
            with self.subTest(source=source):
                with self.assertRaises(ValueError): notes.extract(source,version)
    def test_current_package_has_a_stable_changelog_entry(self):
        import tomllib
        version=tomllib.loads((ROOT/'Cargo.toml').read_text(encoding='utf-8'))['package']['version']
        self.assertIn(f'SYSU NetAuth v{version}',notes.extract((ROOT/'CHANGELOG.md').read_text(encoding='utf-8'),version))
