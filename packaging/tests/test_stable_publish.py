"""Exercise stable publication offline with a fake release asset store."""

import os
import json
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]


class StablePublishTests(unittest.TestCase):
    def test_unsigned_rerun_removes_old_manifest_before_replacing_installers(self):
        workflow = (ROOT / ".github/workflows/package.yml").read_text()
        block = workflow.split("      - name: Publish the stable release\n", 1)[1]
        script = block.split("        run: |\n", 1)[1].split("\n      - name: ", 1)[0]
        script = "\n".join(line[10:] for line in script.splitlines())
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "dist").mkdir()
            (root / "dist/installer").write_text("new")
            (root / "release-tools/packaging").mkdir(parents=True)
            (root / "release-tools/packaging/release-notes.sh").write_text("exit 0\n")
            (root / "assets").mkdir()
            (root / "assets/update-stable.json").write_text("old")
            (root / "gh").write_text('''#!/bin/bash
set -eu
case "$2" in
  view) if [[ "$*" == *--json* ]]; then ls assets; fi ;;
  delete-asset) rm "assets/$4" ;;
  upload) [[ ! -e assets/update-stable.json ]] || exit 42 ;;
  edit) ;;
  *) exit 43 ;;
esac
''')
            (root / "gh").chmod(0o755)
            env = dict(os.environ, PATH=f"{root}:{os.environ['PATH']}",
                       UPDATE_MANIFEST=json.loads((ROOT / "packaging/release-assets.json").read_text())["update_manifest"], RELEASE_TAG="v-test", RUNNER_TEMP=str(root), GITHUB_WORKSPACE=str(root))
            result = subprocess.run(["bash", "-eu", "-c", script], cwd=root,
                                    env=env, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertFalse((root / "assets/update-stable.json").exists())
