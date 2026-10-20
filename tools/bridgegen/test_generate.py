"""Regression coverage for bridge contract value sources and validation."""

import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from generate import ROOT, constant_value, declarations


class ContractTests(unittest.TestCase):
    def setUp(self):
        """Load the real contract for each independent mutation."""
        self.manifest = json.loads((ROOT / "crates/bridge/data/contract.json").read_text(encoding="utf-8"))

    def test_check_never_uses_the_locale_encoding(self):
        """Windows' locale encoding misreads the generated non-ASCII docs, so every file access must name UTF-8."""
        result = subprocess.run(
            [sys.executable, "-X", "warn_default_encoding", "-W", "error::EncodingWarning",
             str(Path(__file__).with_name("generate.py")), "--check"],
            capture_output=True, text=True, encoding="utf-8")
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_game_version_reads_updated_target(self):
        """A target update must change the resolved version without editing the contract."""
        row = next(row for row in self.manifest["constants"] if row["rust_name"] == "GAME_VERSION")
        target = json.loads((ROOT / row["source"]["path"]).read_text(encoding="utf-8"))
        target["game_version"] += ".updated"
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / row["source"]["path"]
            source.parent.mkdir(parents=True)
            source.write_text(json.dumps(target))
            self.assertEqual(constant_value(row, root, {}), target["game_version"])

    def test_port_text_reads_updated_numeric_port(self):
        """The string form follows the numeric port rather than another literal."""
        row = next(row for row in self.manifest["constants"]
                   if row["rust_name"] == "DEFAULT_BEDROCK_PORT_TEXT")
        source_name = row["from_constant"]
        port = next(row["value"] for row in self.manifest["constants"]
                    if row["rust_name"] == source_name)
        self.assertEqual(constant_value(row, ROOT, {source_name: port + 1}), str(port + 1))

    def test_duplicate_enum_spellings_are_rejected(self):
        """Two variants cannot silently claim the same wire value."""
        variants = self.manifest["enums"][0]["variants"]
        variants[1]["value"] = variants[0]["value"]
        with self.assertRaises(ValueError):
            declarations(self.manifest)

    def test_record_lists_require_a_declared_record(self):
        """Reject list element names that cannot be emitted in both languages."""
        self.manifest["structs"][-1]["fields"][0]["type"] = "list:MissingRecord"
        with self.assertRaises(ValueError):
            declarations(self.manifest)

    def test_empty_enum_spelling_is_supported_without_empty_constants(self):
        """Idle progress has an empty wire tag; named string constants still require a value."""
        manifest = copy.deepcopy(self.manifest)
        stage = next(row for row in manifest["enums"] if row["rust_name"] == "ConnectStage")
        stage["variants"] = [row for row in stage["variants"] if row["rust_name"] != "Idle"]
        stage["variants"].insert(0, {"rust_name": "Idle", "go_name": "ConnectStageIdle", "value": ""})
        declarations(manifest)
        row = next(row for row in manifest["constants"] if row["rust_name"] == "RELAY_EVENT_WELCOME")
        row["value"] = ""
        with self.assertRaises(ValueError):
            declarations(manifest)

    def test_wire_values_remain_bounded(self):
        """Reject zero frame limits, port overflow, and signed error overflow."""
        for name, value in [("MAX_FRAME_LEN", 0), ("DEFAULT_BEDROCK_PORT", 65536),
                            ("RPC_INVALID_PARAMS", -2**63 - 1)]:
            with self.subTest(name=name):
                manifest = copy.deepcopy(self.manifest)
                row = next(row for row in manifest["constants"] if row["rust_name"] == name)
                row["value"] = value
                with self.assertRaises(ValueError):
                    declarations(manifest)


if __name__ == "__main__":
    unittest.main()
