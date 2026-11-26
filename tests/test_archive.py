"""PTY regressions for ZIP confirmation and visible asynchronous activity."""

import os
from pathlib import Path
import shlex
import shutil
import time

from test_search_escape import SaltPTYTest


class ArchiveTest(SaltPTYTest):
    def prepare(self):
        root = Path(self.root.name)
        (root / "folder").mkdir()
        (root / "folder" / "nested.txt").write_text("contents")
        bin_dir = root / ".bin"
        bin_dir.mkdir()
        zip_binary = shutil.which("zip")
        if not zip_binary:
            self.skipTest("zip is required")
        wrapper = bin_dir / "zip"
        wrapper.write_text(
            "#!/bin/sh\nsleep 2\n"
            'if [ -f "$HOME/.fail" ]; then echo "test zip failure" >&2; exit 1; fi\n'
            f"exec {shlex.quote(zip_binary)} \"$@\"\n"
        )
        wrapper.chmod(0o755)
        return {"PATH": f"{bin_dir}:{os.environ['PATH']}"}

    def open_confirmation(self):
        self.send(b"/folder\r")
        self.assertIn(b"Create ZIP archive", self.send(b"gc"))

    def wait_for_result(self, expected):
        output = b""
        deadline = time.monotonic() + 8
        while expected not in output and time.monotonic() < deadline:
            output += self.drain()
        self.assertIn(expected, output)
        return output

    def test_confirmation_requires_y_and_cancellation_preserves_sources(self):
        self.open_confirmation()
        self.send(b"\r")
        root = Path(self.root.name)
        self.assertFalse((root / "folder.zip").exists())
        for cancel in (b"n", b"\x1b"):
            self.assertIn(b"Compression cancelled", self.send(cancel))
            self.assertFalse((root / "folder.zip").exists())
            self.assertTrue((root / "folder/nested.txt").exists())
            self.assertIn(b"Create ZIP archive", self.send(b"gc"))

    def test_activity_is_repainted_and_completion_reported(self):
        self.open_confirmation()
        output = self.send(b"y")
        self.assertIn(b"Creating ZIP archive", output)
        self.assertIn(b"Compressing folder.zip", output)
        self.assertIn(b"elapsed", output)
        # Input cannot close Salt or navigate away while ZIP is running.
        self.send(b"q\x1b")
        self.assertIsNone(self.process.poll())
        self.assertTrue(self.drain(), "Activity should keep repainting")
        self.wait_for_result(b"Created:")
        root = Path(self.root.name)
        self.assertEqual((root / "folder.zip").read_bytes()[:2], b"PK")
        self.assertTrue((root / "folder/nested.txt").exists())

    def test_failure_is_reported_and_ui_recovers(self):
        Path(self.root.name, ".fail").touch()
        self.open_confirmation()
        self.assertIn(b"Creating ZIP archive", self.send(b"y"))
        output = self.wait_for_result(b"Could not compress:")
        self.assertIn(b"test zip failure", output)
        self.assertFalse(Path(self.root.name, "folder.zip").exists())
        self.assertIn(b"Create ZIP archive", self.send(b"gc"))
