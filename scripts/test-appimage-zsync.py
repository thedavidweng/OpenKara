import hashlib
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


class DeltaTests(unittest.TestCase):
    script = Path(__file__).with_name('prepare-appimage-zsync.sh')
    transport = 'gh-releases-zsync|thedavidweng|OpenKara|latest|OpenKara_*_amd64.AppImage.zsync'

    def make_image(self, directory, transport):
        image = Path(directory) / 'OpenKara_1_amd64.AppImage'
        image.write_text(f"#!/bin/sh\necho '{transport}' >&2\n" + '# fixture block\n' * 4096)
        image.chmod(0o755)
        return image

    def run_gate(self, directory):
        return subprocess.run(['bash', str(self.script), directory], capture_output=True, text=True)

    def test_rebuilds_stale_control_from_final_bytes_and_roundtrips(self):
        with tempfile.TemporaryDirectory(prefix='appimage delta ') as directory:
            image = self.make_image(directory, self.transport)
            control = image.with_name(image.name + '.zsync')
            subprocess.run(['zsyncmake', '-o', str(control), str(image)], check=True, capture_output=True)
            image.write_text(image.read_text() + '# final bytes\n')
            result = self.run_gate(directory)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            header = control.read_bytes().split(b'\n\n', 1)[0].decode()
            fields = dict(line.split(': ', 1) for line in header.splitlines() if ': ' in line)
            self.assertEqual(fields['SHA-1'], hashlib.sha1(image.read_bytes()).hexdigest())
            self.assertEqual(fields['Filename'], image.name)
            self.assertEqual(fields['URL'], image.name)

    def test_rejects_wrong_update_transport(self):
        with tempfile.TemporaryDirectory() as directory:
            image = self.make_image(directory, 'zsync|https://example.invalid/wrong.zsync')
            result = self.run_gate(directory)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('AppImageUpdate transport mismatch', result.stderr)
            self.assertFalse(image.with_name(image.name + '.zsync').exists())

    def test_rejects_header_only_generator_output(self):
        with tempfile.TemporaryDirectory() as directory:
            image = self.make_image(directory, self.transport)
            control = image.with_name(image.name + '.zsync')
            subprocess.run(['zsyncmake', '-o', str(control), str(image)], check=True, capture_output=True)
            header = Path(directory) / 'header'
            header.write_bytes(control.read_bytes().split(b'\n\n', 1)[0] + b'\n\n')
            tools = Path(directory) / 'tools'
            tools.mkdir()
            generator = tools / 'zsyncmake'
            generator.write_text('#!/bin/sh\ncp "$ZSYNC_HEADER" "$4"\n')
            generator.chmod(0o755)
            result = subprocess.run(
                ['bash', str(self.script), directory], capture_output=True, text=True,
                env={**os.environ, 'PATH': f'{tools}:{os.environ["PATH"]}', 'ZSYNC_HEADER': str(header)},
            )
            self.assertNotEqual(result.returncode, 0)

    def test_rejects_failed_runtime_metadata_query(self):
        with tempfile.TemporaryDirectory() as directory:
            image = self.make_image(directory, self.transport)
            image.write_text(image.read_text() + 'exit 1\n')
            result = self.run_gate(directory)
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse(image.with_name(image.name + '.zsync').exists())

    def test_requires_exactly_one_appimage(self):
        with tempfile.TemporaryDirectory() as directory:
            for count in (0, 2):
                with self.subTest(count=count):
                    for index in range(count):
                        (Path(directory) / f'{index}.AppImage').touch()
                    result = self.run_gate(directory)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn(f'Expected exactly one AppImage, found {count}.', result.stderr)


if __name__ == '__main__':
    unittest.main()
