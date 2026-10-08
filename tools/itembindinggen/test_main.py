"""Regression tests for bounded archive decoding and literal JSON strings."""

import struct
import unittest

from main import archive_entries, item_json


class ArchiveTests(unittest.TestCase):
    def test_archive_bounds_and_duplicate_names(self):
        """Reject truncated tables, payloads, unsupported versions and duplicate names."""
        header = bytes.fromhex("7d2725b1a0527026") + struct.pack("<II", 1, 1)
        row = bytearray(256)
        row[0] = 6
        row[1:7] = b"a.json"
        struct.pack_into("<II", row, 248, 0, 2)
        archive = header + row + b"{}"
        self.assertEqual(archive_entries(archive), {"a.json": b"{}"})
        for invalid in (archive[:15], archive[:-1], archive[:12] + struct.pack("<I", 2) + archive[16:]):
            with self.assertRaises(ValueError):
                archive_entries(invalid)
        duplicate = header[:8] + struct.pack("<II", 2, 1) + row + row + b"{}"
        with self.assertRaises(ValueError):
            archive_entries(duplicate)

    def test_comments_leave_quoted_text_intact(self):
        """Keep comment-like characters inside strings while removing actual comments."""
        raw = b'{/* comment */ "url": "https://example.test/*literal*/", // line\n "value": 1}'
        self.assertEqual(item_json(raw), {"url": "https://example.test/*literal*/", "value": 1})


if __name__ == "__main__":
    unittest.main()
