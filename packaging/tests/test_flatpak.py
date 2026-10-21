"""Check the prepared Flatpak inputs without downloading dependencies or compiling."""

import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("flatpak_prepare", ROOT / "packaging/flatpak/prepare.py")
PREPARE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PREPARE)


class FlatpakTests(unittest.TestCase):
    """Exercise source isolation, toolchain pinning, and desktop identity."""

    def test_snapshot_excludes_untracked_and_ignored_files(self):
        """A source package contains tracked working files, never local runtime inputs."""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / "repo"
            root.mkdir()
            subprocess.run(["git", "init", "-q", str(root)], check=True)
            (root / "source.txt").write_text("initial")
            subprocess.run(["git", "-C", str(root), "add", "source.txt"], check=True)
            (root / "source.txt").write_text("working copy")
            (root / "credentials").write_text("private")
            (root / ".local").mkdir()
            (root / ".local/carrier").write_text("local data")
            destination = Path(temporary) / "snapshot"
            PREPARE.snapshot(root, destination)
            self.assertEqual((destination / "source.txt").read_text(), "working copy")
            self.assertEqual([p.name for p in destination.iterdir()], ["source.txt"])

    def test_rust_components_use_the_pinned_manifest(self):
        """The offline build fetches exactly the upstream-hashed compiler components."""
        target = "x86_64-unknown-linux-gnu"
        packages = {
            name: {"target": {target: {"xz_url": f"https://example.org/{name}", "xz_hash": name}}}
            for name in ("rustc", "rust-std", "cargo")
        }
        module = PREPARE.rust_module(target, {"pkg": packages})
        for source in module["sources"]:
            expected = packages[source["dest"]]["target"][target]
            self.assertEqual(source["url"], expected["xz_url"])
            self.assertEqual(source["sha256"], expected["xz_hash"])
        self.assertEqual(len(module["build-commands"]), len(packages))

    def test_desktop_identity_matches_the_manifest(self):
        """The exported desktop entry and listing refer to the installed application ID."""
        manifest = json.loads(PREPARE.TEMPLATE.read_text())
        app_id = manifest["app-id"]
        metadata = ET.parse(ROOT / f"packaging/flatpak/{app_id}.metainfo.xml").getroot()
        self.assertEqual(metadata.findtext("id"), app_id)
        self.assertEqual(metadata.findtext("launchable"), f"{app_id}.desktop")
        self.assertFalse(any("filesystem=" in arg for arg in manifest["finish-args"]))


if __name__ == "__main__":
    unittest.main()
