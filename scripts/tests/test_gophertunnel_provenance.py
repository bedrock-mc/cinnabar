#!/usr/bin/env python3
"""Hermetic regression checks for the Bash acceptance provenance boundary."""

import copy
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


PROJECT_ROOT = Path(__file__).resolve().parents[2]
MODULE_PATH = "github.com/sandertv/gophertunnel"


class GophertunnelProvenanceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        # Go's manifest parser reads the authoritative owner without fetching modules.
        cls.manifest = json.loads(subprocess.check_output(
            ["go", "-C", str(PROJECT_ROOT / "tools/localserver"), "mod", "edit", "-json"],
            env={**os.environ, "GOWORK": "off"},
        ))
        cls.replacement = next(entry["New"] for entry in cls.manifest["Replace"]
                               if entry["Old"]["Path"] == MODULE_PATH)

    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="gophertunnel provenance ")
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.state_path = self.root / "state.json"
        self.calls_path = self.root / "calls.jsonl"
        self.commit = self.replacement["Version"].split("-")[-1] + "0" * 28
        self.state = {
            "root": str(self.root), "manifest": copy.deepcopy(self.manifest),
            "resolved": {"Path": MODULE_PATH, "Replace": copy.deepcopy(self.replacement)},
            "download": {
                **self.replacement,
                "Origin": {"VCS": "git", "URL": "https://" + self.replacement["Path"],
                           "Hash": self.commit},
            },
        }
        self.bin_dir = self.root / "bin"
        self.bin_dir.mkdir()
        mock_go = self.bin_dir / "go"
        mock_go.write_text('''#!/usr/bin/env python3
import json, os, pathlib, sys
state_path = pathlib.Path(os.environ["GOPHERTUNNEL_TEST_STATE"])
state = json.loads(state_path.read_text())
args = sys.argv[1:]
with state_path.with_name("calls.jsonl").open("a") as calls:
    calls.write(json.dumps(args) + "\\n")
if args == ["-C", state["root"] + "/tools/localserver", "mod", "edit", "-json"]:
    result = state["manifest"]
elif args == ["-C", state["root"] + "/tools/localserver", "list", "-m", "-json", "github.com/sandertv/gophertunnel"]:
    result = state["resolved"]
elif args == ["-C", state["root"] + "/tools/localserver", "mod", "download", "-json",
              state["resolved"]["Replace"]["Path"] + "@" + state["resolved"]["Replace"]["Version"]]:
    result = state["download"]
else:
    sys.exit("unexpected Go command: " + repr(args))
print(result if isinstance(result, str) else json.dumps(result))
''', encoding="utf-8")
        mock_go.chmod(0o755)

    def resolve(self):
        self.state_path.write_text(json.dumps(self.state), encoding="utf-8")
        environment = dict(os.environ, CINNABAR_ACCEPTANCE_TEST_LIBRARY_ONLY="1",
                           GOPHERTUNNEL_TEST_STATE=str(self.state_path))
        environment["PATH"] = str(self.bin_dir) + os.pathsep + environment["PATH"]
        return subprocess.run(
            ["bash", "-c", 'source "$1"; resolve_pinned_gophertunnel_commit "$2"',
             "provenance-test", str(PROJECT_ROOT / "scripts/acceptance.sh"), str(self.root)],
            env=environment, capture_output=True, text=True, check=False, timeout=10,
        )

    def reject(self, message):
        result = self.resolve()
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn(message, result.stderr)
        self.assertEqual(result.stdout, "")

    def expected_entry(self):
        return next(entry for entry in self.state["manifest"]["Replace"]
                    if entry["Old"]["Path"] == MODULE_PATH)

    def test_manifest_pin_resolves_full_origin_commit(self):
        result = self.resolve()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), self.commit)
        calls = [json.loads(line) for line in self.calls_path.read_text().splitlines()]
        self.assertEqual(calls[0], ["-C", str(self.root / "tools/localserver"), "mod", "edit", "-json"])

    def test_changed_manifest_pin_is_followed_without_script_literal(self):
        changed_commit = hashlib.sha1(b"a different fixture pin").hexdigest()
        version = self.replacement["Version"].rsplit("-", 1)[0] + "-" + changed_commit[:12]
        self.expected_entry()["New"]["Version"] = version
        self.state["resolved"]["Replace"]["Version"] = version
        self.state["download"]["Version"] = version
        self.state["download"]["Origin"]["Hash"] = changed_commit
        result = self.resolve()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), changed_commit)

    def test_wrong_manifest_source_fails_before_workspace_query(self):
        self.expected_entry()["New"]["Path"] = "example.invalid/another-fork"
        self.reject("tools/localserver/go.mod pins a different gophertunnel source")
        self.assertEqual(len(self.calls_path.read_text().splitlines()), 1)

    def test_duplicate_or_version_specific_manifest_replacement_rejected(self):
        self.state["manifest"]["Replace"].append(copy.deepcopy(self.expected_entry()))
        self.reject("one unversioned gophertunnel replacement")
        self.state["manifest"]["Replace"].pop()
        self.expected_entry()["Old"]["Version"] = "v1.0.0"
        self.reject("one unversioned gophertunnel replacement")
        self.expected_entry()["Old"]["Version"] = None
        self.reject("one unversioned gophertunnel replacement")

    def test_local_or_unpinned_manifest_replacement_rejected(self):
        for version in (None, "v1.0.0", "v1.0.0-0123456789ab", self.replacement["Version"] + "\n"):
            with self.subTest(version=version):
                self.expected_entry()["New"]["Version"] = version
                self.reject("not pinned to a commit pseudo-version")

    def test_workspace_version_and_source_must_match_owner(self):
        for field, value in (("Version", "v1.0.0"), ("Path", "example.invalid/another-fork")):
            with self.subTest(field=field):
                self.state["resolved"]["Replace"] = dict(self.replacement, **{field: value})
                self.reject("different gophertunnel module or replacement version")

    def test_origin_commit_url_and_vcs_verified(self):
        original = copy.deepcopy(self.state["download"]["Origin"])
        for field, value in (("Hash", "0" * 40), ("Hash", self.commit[:12]), ("Hash", self.commit + "\n"),
                             ("Hash", self.commit.upper()), ("URL", "https://example.invalid/fork"),
                             ("VCS", "hg")):
            with self.subTest(field=field, value=value):
                self.state["download"]["Origin"] = dict(original, **{field: value})
                self.reject("origin does not match the expected exact commit")

    def test_download_identity_must_match_owner(self):
        for field, value in (("Version", "v1.0.0"), ("Path", "example.invalid/another-fork")):
            with self.subTest(field=field):
                self.state["download"][field] = value
                self.reject("origin does not match the expected exact commit")
                self.state["download"][field] = self.replacement[field]

    def test_malformed_and_duplicate_json_rejected(self):
        for field, value in (("manifest", "not JSON"), ("resolved", '{"Path":"x","Path":"y"}'),
                             ("download", "[]")):
            with self.subTest(field=field):
                original = self.state[field]
                self.state[field] = value
                self.reject("gophertunnel")
                self.state[field] = original


if __name__ == "__main__":
    unittest.main()
