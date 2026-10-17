import hashlib
from pathlib import Path
import subprocess
import tempfile
import unittest


class DeltaTests(unittest.TestCase):
    def test_mismatched_final_bytes_are_rejected(self):
        script = Path(__file__).with_name('collect-appimage-zsync.sh')
        with tempfile.TemporaryDirectory() as directory:
            image = Path(directory) / 'OpenKara_1_amd64.AppImage'
            image.write_text("#!/bin/sh\necho 'gh-releases-zsync|thedavidweng|OpenKara|latest|OpenKara_*_amd64.AppImage.zsync' >&2\n")
            image.chmod(0o755)
            image.with_name(image.name+'.zsync').write_text(f'Filename: {image.name}\nSHA-1: {hashlib.sha1(image.read_bytes()).hexdigest()}\n\n')
            subprocess.run(['bash',str(script),directory],check=True)
            image.write_text(image.read_text()+'\n')
            result=subprocess.run(['bash',str(script),directory],capture_output=True,text=True)
            self.assertNotEqual(result.returncode,0)
            self.assertIn('zsync does not describe',result.stderr)


if __name__ == '__main__':
    unittest.main()
