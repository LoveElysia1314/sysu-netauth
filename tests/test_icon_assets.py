"""Shared source, generated geometry and native Windows container regression checks."""
import importlib.util
import json
from pathlib import Path
import re
import struct
import unittest
import zlib
import tempfile
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
module = importlib.util.spec_from_file_location('generate_icons', ROOT / 'scripts/generate-icons.py')
generator = importlib.util.module_from_spec(module)
module.loader.exec_module(generator)

class IconAssetsTests(unittest.TestCase):
    def test_source_line_endings_do_not_change_generated_geometry_or_provenance(self):
        expected = generator.outputs()
        manifest = generator.windows_manifest()
        with tempfile.TemporaryDirectory() as temporary:
            source = Path(temporary) / 'source.json'
            source.write_bytes(generator.source_bytes().replace(b'\n', b'\r\n'))
            with patch.object(generator, 'SOURCE', source):
                self.assertEqual(generator.outputs(), expected)
                self.assertEqual(generator.windows_manifest(), manifest)
    def test_generated_text_and_provenance(self):
        for path, expected in generator.outputs().items():
            with self.subTest(path=path.name):
                self.assertEqual(path.read_text(encoding='utf-8'), expected)
        manifest = json.loads((generator.GENERATED/'windows/manifest.json').read_text(encoding='utf-8'))
        self.assertEqual(manifest, generator.windows_manifest())
        self.assertEqual(len(manifest['files']), 19)

    def test_ico_sizes_offsets_and_png_integrity(self):
        for path in sorted((generator.GENERATED/'windows').glob('*.ico')):
            with self.subTest(path=path.name):
                data = path.read_bytes()
                reserved, kind, count = struct.unpack_from('<HHH', data)
                self.assertEqual((reserved, kind, count), (0, 1, len(generator.SIZES)))
                expected_offset = 6 + count * 16
                for index, size in enumerate(generator.SIZES):
                    w, h, colors, zero, planes, bits, length, offset = struct.unpack_from('<BBBBHHII', data, 6 + index * 16)
                    self.assertEqual((w or 256,h or 256,colors,zero,planes,bits), (size,size,0,0,1,32))
                    self.assertEqual(offset, expected_offset)
                    png = data[offset:offset+length]
                    self.assertEqual(len(png), length)
                    self.assertEqual(png[:8], b'\x89PNG\r\n\x1a\n')
                    self.assertEqual(struct.unpack_from('>II',png,16), (size,size))
                    self.assertEqual(png[24:26], bytes([8,6]))  # RGBA8
                    position = 8
                    while position < len(png):
                        n = struct.unpack_from('>I',png,position)[0]
                        chunk = png[position+4:position+8+n]
                        crc = struct.unpack_from('>I',png,position+8+n)[0]
                        self.assertEqual(zlib.crc32(chunk) & 0xffffffff, crc)
                        position += 12+n
                    self.assertEqual(position, len(png))
                    expected_offset += length
                self.assertEqual(expected_offset, len(data))

    def test_windows_resource_ids_preserve_existing_state_mapping(self):
        rc = (ROOT/'packaging/windows.rc').read_text(encoding='utf-8')
        actual = {int(i): p for i,p in re.findall(r'(\d+) ICON "([^"]+)"', rc)}
        for offset, suffix in [(10, 'light'), (20, 'dark')]:
            for index, name in enumerate(['ready','disconnected','authenticated','authenticating','failed','unavailable'],2):
                self.assertEqual(actual[offset+index], f'../assets/icons/generated/windows/{name}-{suffix}.ico')
        for index, name in enumerate(['app','ready','disconnected','authenticated','authenticating','failed','unavailable'],1):
            self.assertEqual(actual[index], f'../assets/icons/generated/windows/{name}.ico')
            self.assertTrue((ROOT/'packaging'/actual[index]).is_file())
        installer = (ROOT/'scripts/setup.template.iss').read_text(encoding='utf-8')
        self.assertIn('SetupIconFile=..\\assets\\icons\\generated\\windows\\app.ico', installer)
        self.assertFalse((ROOT/'assets/icon-ethernet.ico').exists())

if __name__ == '__main__': unittest.main()
