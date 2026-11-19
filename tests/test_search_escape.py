"""PTY regression: cargo build --locked && python3 -m unittest discover -s tests."""

import fcntl
import os
from pathlib import Path
import pty
import select
import struct
import subprocess
import tempfile
import termios
import time
import unittest


class SearchEscapeTest(unittest.TestCase):
    def setUp(self):
        self.root = tempfile.TemporaryDirectory()
        self.addCleanup(self.root.cleanup)
        Path(self.root.name, "sample.txt").touch()
        self.master, slave = pty.openpty()
        self.addCleanup(os.close, self.master)
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
        binary = os.environ.get(
            "SALT_TEST_BINARY",
            str(Path(__file__).resolve().parents[1] / "target/debug/salt"),
        )
        try:
            self.process = subprocess.Popen(
                [str(binary), self.root.name],
                stdin=slave, stdout=slave, stderr=slave,
                env={**os.environ, "TERM": "xterm-256color", "HOME": self.root.name},
            )
        finally:
            os.close(slave)
        self.addCleanup(self.stop)
        output = b""
        deadline = time.monotonic() + 5
        while b"NORMAL" not in output and time.monotonic() < deadline:
            output += self.drain()
        self.assertIn(b"NORMAL", output, "Salt did not open its directory view")

    def stop(self):
        if self.process.poll() is None:
            self.process.terminate()
        self.process.wait(timeout=5)

    def drain(self):
        output = b""
        deadline = time.monotonic() + 0.4
        while time.monotonic() < deadline:
            if select.select([self.master], [], [], 0.05)[0]:
                output += os.read(self.master, 65536)
        return output

    def send(self, keys):
        os.write(self.master, keys)
        return self.drain()

    def confirm_search(self):
        self.assertIn(b"48;5;2m", self.send(b"/sam"))
        self.send(b"\r")

    def assert_highlight_removed(self, output):
        # The cursor covers 's'; the highlighted 'am' must be repainted in
        # the default background, not merely hidden by clearing the prompt.
        self.assertIn(b"\x1b[2;5Ham", output)
        self.assertNotIn(b"48;5;2m", output)

    def test_escape_clears_confirmed_search(self):
        self.confirm_search()
        self.assert_highlight_removed(self.send(b"\x1b"))
        self.assertNotIn(b"48;5;2m", self.send(b"n"))

    def test_cancel_search_does_not_restore_old_highlights(self):
        self.confirm_search()
        self.send(b"/sam")
        self.assert_highlight_removed(self.send(b"\x1b"))
