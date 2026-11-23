"""PTY regressions for immediate deletion of Space-marked entries."""

from pathlib import Path
import shlex

from test_search_escape import SaltPTYTest


class MarkedDeleteTest(SaltPTYTest):
    def prepare(self):
        root = Path(self.root.name)
        (root / "a file.txt").touch()
        (root / "b folder").mkdir()
        (root / "b folder" / "nested.txt").touch()
        self.marker = root / ".opened"
        editor = root / ".editor"
        editor.write_text('printf "%s" "$1" > "$HOME/.opened"\n')
        return {"VISUAL": f"sh {shlex.quote(str(editor))}"}

    def mark_and_delete(self):
        self.send(b"gg  ")
        output = self.send(b"d")
        self.assertIn(b"Delete permanently?", output)
        self.assert_sources_exist()

    def assert_sources_exist(self):
        root = Path(self.root.name)
        self.assertTrue((root / "a file.txt").exists())
        self.assertTrue((root / "b folder" / "nested.txt").exists())
        self.assertTrue((root / "sample.txt").exists())

    def test_confirm_deletes_immediately_and_enter_opens_remaining_file(self):
        self.mark_and_delete()
        # Enter must not authorize a destructive operation.
        self.send(b"\r")
        self.assert_sources_exist()
        self.assertIn(b"Deleted 2 item(s)", self.send(b"y"))
        root = Path(self.root.name)
        self.assertFalse((root / "a file.txt").exists())
        self.assertFalse((root / "b folder").exists())
        self.assertTrue((root / "sample.txt").exists())
        output = self.send(b"\r")
        self.assertNotIn(b"Save or discard", output)
        self.assertEqual(Path(self.marker.read_text()), (root / "sample.txt").resolve())
        self.send(b"q")
        self.assertEqual(self.process.wait(timeout=5), 0)

    def test_no_and_escape_preserve_marks_and_allow_retry(self):
        self.mark_and_delete()
        for cancel in (b"n", b"\x1b"):
            self.send(cancel)
            self.assert_sources_exist()
            # Marks and rows survive cancellation, so d reopens the same dialog.
            self.assertIn(b"Delete permanently?", self.send(b"d"))
        self.send(b"n")
        self.assertIn(b"No changes", self.send(b":w\r"))
        self.send(b"\x1bq")
        self.assertEqual(self.process.wait(timeout=5), 0)

    def test_pending_edits_block_immediate_deletion(self):
        self.send(b"iX\x1b")
        self.send(b"gg  ")
        self.assertIn(b"Save or discard changes before deleting", self.send(b"d"))
        self.assert_sources_exist()
        self.assertNotIn(b"Delete permanently?", self.send(b"y"))
        self.assert_sources_exist()

    def test_partial_failure_reloads_actual_disk_state(self):
        self.mark_and_delete()
        root = Path(self.root.name)
        # Default sorting puts the directory first. Remove the second target
        # externally while the confirmation keeps automatic reload suspended.
        (root / "a file.txt").unlink()
        output = self.send(b"y")
        self.assertIn(b"Deleted 1/2 item(s)", output)
        self.assertIn(b"Could not delete", output)
        self.assertFalse((root / "b folder").exists())
        self.assertTrue((root / "sample.txt").exists())
        self.send(b"\r")
        self.assertEqual(Path(self.marker.read_text()), (root / "sample.txt").resolve())

    def test_dd_still_deletes_only_current_entry(self):
        self.send(b"/a file\r")
        self.assertIn(b"Delete permanently?", self.send(b"dd"))
        self.send(b"y")
        root = Path(self.root.name)
        self.assertFalse((root / "a file.txt").exists())
        self.assertTrue((root / "b folder" / "nested.txt").exists())
        self.assertTrue((root / "sample.txt").exists())
