"""Exercise the Cinnabar API release notice offline with a fake curl."""

import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("manifest_publish", Path(__file__).with_name("test_manifest_publish.py"))
manifest_publish = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(manifest_publish)
STEP = "Announce the release to the Cinnabar API"
SECRET = "hook-secret-value"
FAKE_CURL = '''#!/bin/bash
printf '%s\\n' "$@" > argv
previous=
for argument in "$@"; do
  if [[ "$previous" == -H && "$argument" == @* ]]; then cat "${argument#@}" >> headers; fi
  [[ "$previous" != --data ]] || printf '%s' "$argument" > body
  previous="$argument"
done
printf '%s' "${FAKE_STATUS:-204}"
'''


class ReleaseHookTests(unittest.TestCase):
    def setUp(self):
        """Builds a workspace whose release tooling holds the shared contract manifest."""
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        contract = self.root / "release-tools/crates/bridge/data/contract.json"
        contract.parent.mkdir(parents=True)
        contract.write_text((ROOT / "crates/bridge/data/contract.json").read_text())
        (self.root / "curl").write_text(FAKE_CURL)
        (self.root / "curl").chmod(0o755)

    def announce(self, secret=SECRET, status="204", published="true"):
        """Runs the workflow step against the fake curl."""
        env = dict(os.environ, PATH=f"{self.root}:{os.environ['PATH']}", GITHUB_WORKSPACE=str(self.root),
                   RELEASE_CHANNEL="nightly", RELEASE_VERSION="0.1.18", GITHUB_RUN_NUMBER="42", MANIFEST_PUBLISHED=published, FAKE_STATUS=status,
                   CINNABAR_API_RELEASE_HOOK_SECRET=secret)
        result = subprocess.run(["bash", "-e", "-c", manifest_publish.step(STEP)], cwd=self.root, env=env,
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        return result

    def test_posts_the_published_channel_without_exposing_the_secret(self):
        result = self.announce()
        base = next(row["value"] for row in json.loads((ROOT / "crates/bridge/data/contract.json").read_text())["constants"]
                    if row["go_name"] == "CinnabarAPIBaseURL")
        argv = (self.root / "argv").read_text()
        self.assertIn(f"{base}/v1/releases", argv.splitlines())
        self.assertNotIn(SECRET, argv)
        self.assertNotIn(SECRET, result.stdout + result.stderr)
        self.assertEqual((self.root / "headers").read_text().strip(), f"Authorization: Bearer {SECRET}")
        self.assertEqual(json.loads((self.root / "body").read_text()), {"channel": "nightly", "version": "0.1.18", "build": 42})

    def test_a_failed_notice_warns_without_failing_the_release(self):
        result = self.announce(status="500")
        self.assertIn("::warning::Cinnabar API release notice failed (HTTP 500)", result.stdout)

    def test_an_unpublished_manifest_does_not_announce(self):
        """The step guards its own execution as well as the workflow's output condition."""
        self.announce(published="")
        self.assertFalse((self.root / "argv").exists())

    def test_a_missing_secret_skips_the_notice(self):
        result = self.announce(secret="")
        self.assertFalse((self.root / "argv").exists())
        self.assertIn("::warning::", result.stdout)


if __name__ == "__main__":
    unittest.main()
