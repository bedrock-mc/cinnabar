"""Inject local carriers into a fake unsigned IPA as packaging/ios/add-assets.py does for users."""

import json
from pathlib import Path
import shutil
import stat
import subprocess
import sys
import tempfile
import unittest
import zipfile


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "packaging/ios/add-assets.py"
CACHE = json.loads((ROOT / "assets/vanilla-source.json").read_text())["cache_dir"]
PACK = f"{CACHE.removeprefix('.local/')}/resource_pack"
APP = "Payload/Cinnabar.app/"
RESOURCES = APP + json.loads((ROOT / "packaging/ios/runtime.json").read_text())["resource_dir"] + "/"


def entry(name, mode):
    info = zipfile.ZipInfo(name, (2026, 1, 1, 0, 0, 0))
    info.create_system = 3
    info.external_attr = mode << 16
    return info


class AddAssetsTests(unittest.TestCase):
    def setUp(self):
        self.temp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.temp)
        self.local = self.temp / ".local"
        self.ipa = self.temp / "Cinnabar-1.0.0-unsigned.ipa"
        with zipfile.ZipFile(self.ipa, "w", zipfile.ZIP_DEFLATED) as ipa:
            ipa.writestr(entry(APP, stat.S_IFDIR | 0o755), b"")
            ipa.writestr(entry(APP + "Cinnabar", stat.S_IFREG | 0o755), b"\xcf\xfa\xed\xfe binary")
            ipa.writestr(entry(RESOURCES + "fonts/ui.ttf", stat.S_IFREG | 0o644), b"font")
            ipa.writestr(entry(RESOURCES + "assets/compiled/stale.bin", stat.S_IFREG | 0o644), b"old")

    def write_local(self, relative, content):
        path = self.local / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content)

    def run_script(self, *args):
        return subprocess.run(
            [sys.executable, str(SCRIPT), str(self.ipa), "--local", str(self.local), *args],
            capture_output=True,
            text=True,
        )

    def test_carriers_and_pack_join_the_app_resources_beside_the_original_entries(self):
        self.write_local("assets/compiled/terrain/atlas.bin", b"atlas")
        self.write_local(f"{PACK}/ui/_global_variables.json", b"{}")
        self.write_local("assets/unrelated.txt", b"not injected")

        result = self.run_script()

        self.assertEqual(result.returncode, 0, result.stderr)
        out = self.temp / "Cinnabar-1.0.0-assets.ipa"
        self.assertIn(str(out), result.stdout)
        with zipfile.ZipFile(out) as ipa:
            self.assertEqual(
                sorted(ipa.namelist()),
                sorted([
                    APP,
                    APP + "Cinnabar",
                    RESOURCES + "fonts/ui.ttf",
                    RESOURCES + "assets/compiled/terrain/atlas.bin",
                    RESOURCES + f"{PACK}/ui/_global_variables.json",
                ]),
            )
            self.assertEqual(ipa.read(RESOURCES + "assets/compiled/terrain/atlas.bin"), b"atlas")
            self.assertEqual(ipa.read(APP + "Cinnabar"), b"\xcf\xfa\xed\xfe binary")
            self.assertEqual(stat.S_IMODE(ipa.getinfo(APP + "Cinnabar").external_attr >> 16), 0o755)

    def test_missing_pack_names_the_directory_and_make_assets(self):
        self.write_local("assets/compiled/terrain/atlas.bin", b"atlas")

        result = self.run_script()

        self.assertEqual(result.returncode, 1)
        self.assertIn(str(Path(PACK)), result.stderr)
        self.assertIn("make assets", result.stderr)
        self.assertFalse((self.temp / "Cinnabar-1.0.0-assets.ipa").exists())

    def test_an_existing_output_is_kept_unless_forced(self):
        self.write_local("assets/compiled/terrain/atlas.bin", b"atlas")
        self.write_local(f"{PACK}/manifest.json", b"{}")
        out = self.temp / "out.ipa"
        out.write_bytes(b"keep")

        refused = self.run_script("--out", str(out))
        forced = self.run_script("--out", str(out), "--force")

        self.assertEqual(refused.returncode, 1)
        self.assertIn("--force", refused.stderr)
        self.assertEqual(forced.returncode, 0, forced.stderr)
        with zipfile.ZipFile(out) as ipa:
            self.assertIn(RESOURCES + f"{PACK}/manifest.json", ipa.namelist())


if __name__ == "__main__":
    unittest.main()
