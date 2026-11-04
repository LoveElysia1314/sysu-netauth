"""Release contracts: installers only, matching provenance, no obsolete payloads."""
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location("prepare_release_assets", ROOT / "scripts/prepare_release_assets.py")
assets = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(assets)

class ReleaseAssetsTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="netauth release assets ")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.info = {"tag":"v0.7.0-beta.3", "version":"0.7.0-beta.3", "commit":"expected-commit"}
        self.metadata = self.root / "release-version.json"
        self.metadata.write_text(json.dumps(self.info),encoding="utf-8")
        self.output = self.root / "output"
        self.output.mkdir()
        for label in ("windows_x64", "windows_arm64"):
            name=f"SYSUNetAuth_{label}_Setup_v{self.info['version']}.exe"
            path=self.output/name
            path.write_bytes(b"MZ fixture "+name.encode())
            (self.output/f"SHA256SUMS-{label.lower().replace('_','-')}.txt").write_text(f"{assets.digest(path)}  {name}\n")
        self.linux=self.root/'linux.tar.gz'
        self.write_linux(self.info)
        self.macos={arch:self.root/f'macos-{arch}.pkg' for arch in ('arm64','x64')}
        self.provenance={arch:self.root/f'macos-{arch}.json' for arch in self.macos}
        for arch in self.macos: self.write_macos(arch,self.info)

    def write_linux(self,info,omit=None):
        with tarfile.open(self.linux,'w:gz') as archive:
            for name in ('sysu_netauth','sysu_netauth_service','sysu-netauth.service','install-linux.sh','release-version.json'):
                if name==omit: continue
                payload=json.dumps(info).encode() if name.endswith('.json') else b'fixture binary or installation script'
                member=tarfile.TarInfo(f'sysu-netauth-linux-x64/{name}')
                member.size=len(payload);member.mode=0o755
                archive.addfile(member,io.BytesIO(payload))

    def write_macos(self,arch,info):
        path=self.macos[arch];path.write_bytes(b'xar! fixture installer')
        self.provenance[arch].write_text(json.dumps({'release':info,'arch':arch,'sha256':assets.digest(path)}))

    def prepare(self,platforms=None,output=None):
        return assets.prepare(self.metadata,self.linux,output or self.output,macos_inputs=self.macos,macos_metadata=self.provenance,platforms=platforms)

    def required(self): return {name for name,tier in assets.PLATFORM_TIERS.items() if tier<=2}

    def test_five_installers_and_five_checksums_no_portable_archives(self):
        result=self.prepare()
        self.assertEqual(len(result),10)
        self.assertEqual(sum(p.suffix=='.exe' for p in result),2)
        self.assertEqual(sum(p.suffix=='.pkg' for p in result),2)
        self.assertFalse(any('Portable' in p.name or p.suffix=='.zip' for p in result))
        for arch,path in self.macos.items():
            published=self.output/f"SYSUNetAuth_macos_{arch}_v{self.info['version']}.pkg"
            self.assertEqual(published.read_bytes(),path.read_bytes())

    def test_missing_windows_arm64_blocks_release(self):
        for path in self.output.glob('*arm64*'): path.unlink()
        with self.assertRaisesRegex(ValueError,'missing=.*windows_arm64'): self.prepare()

    def test_portable_and_gnu_installers_are_rejected(self):
        for name in ('SYSUNetAuth_Portable.zip','SYSUNetAuth_Rust_gnu_x64_Setup.exe'):
            path=self.output/name;path.write_bytes(b'obsolete')
            with self.assertRaisesRegex(ValueError,'unexpected='): self.prepare()
            path.unlink()

    def test_windows_checksum_mismatch_blocks_release(self):
        next(self.output.glob('*.exe')).write_bytes(b'corrupt')
        with self.assertRaisesRegex(ValueError,'Checksum mismatch'): self.prepare()

    def test_linux_commit_mismatch_blocks_release(self):
        self.write_linux(self.info|{'commit':'another'})
        with self.assertRaisesRegex(ValueError,'linux-x64 artifact version/commit'): self.prepare()

    def test_linux_install_script_is_mandatory(self):
        self.write_linux(self.info,omit='install-linux.sh')
        with self.assertRaises(KeyError): self.prepare()

    def test_missing_macos_blocks_release(self):
        for path in self.macos.values(): path.unlink()
        with self.assertRaises(FileNotFoundError): self.prepare()

    def test_architecture_specific_collection_keeps_matching_checksums(self):
        for arch in self.macos:
            result=self.prepare(platforms={f'macos-{arch}'},output=self.root/f'macos-{arch}')
            self.assertEqual({p.suffix for p in result},{'.pkg','.txt'})

    def test_macos_metadata_architecture_and_commit_are_verified(self):
        for replacement in ({'release':self.info|{'commit':'another'}},{'arch':'x64'}):
            self.write_macos('arm64',self.info)
            data=json.loads(self.provenance['arm64'].read_text());data.update(replacement)
            self.provenance['arm64'].write_text(json.dumps(data))
            with self.assertRaisesRegex(ValueError,'macos-arm64 artifact version/commit'): self.prepare()

    def test_macos_package_digest_and_format_are_verified(self):
        self.macos['arm64'].write_bytes(b'xar! changed payload')
        with self.assertRaisesRegex(ValueError,'installer checksum mismatch'): self.prepare()
        self.macos['arm64'].write_bytes(b'not a package')
        with self.assertRaisesRegex(ValueError,'not a flat installer'): self.prepare()

    def test_missing_package_metadata_blocks_publication(self):
        with self.assertRaisesRegex(ValueError,'Missing macOS installer metadata'):
            assets.prepare(self.metadata,self.linux,self.output,macos_inputs=self.macos)

    def test_cli_requires_complete_five_platform_release(self):
        command=[sys.executable,str(ROOT/'scripts/prepare_release_assets.py'),'--metadata',str(self.metadata),'--linux-input',str(self.linux),'--output-dir',str(self.output)]
        for arch in self.macos:
            command += [f'--macos-{arch}-input',str(self.macos[arch]),f'--macos-{arch}-metadata',str(self.provenance[arch])]
        result=subprocess.run(command,check=True,capture_output=True,text=True)
        self.assertEqual(len(result.stdout.splitlines()),10)

    def test_empty_or_unknown_platform_selection_is_rejected(self):
        for platforms in (set(),{'unknown'}):
            with self.assertRaisesRegex(ValueError,'Invalid release platform selection'): self.prepare(platforms=platforms)

if __name__=='__main__': unittest.main()
