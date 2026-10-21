"""Link Android resources without compiling the client or running asset preparation."""

import json
import os
from pathlib import Path
import re
import shutil
import struct
import subprocess
import tempfile
import unittest
import zipfile


ROOT = Path(__file__).resolve().parents[2]


class AndroidResourcesTests(unittest.TestCase):
    def test_launcher_and_application_resolve_to_a_packaged_product_icon(self):
        """Android's package metadata must resolve the launcher icon to a real PNG in the APK."""
        sdk = os.environ.get("ANDROID_HOME") or os.environ.get("ANDROID_SDK_ROOT")
        if not sdk:
            self.skipTest("missing fixture: Android SDK (ANDROID_HOME)")
        pins = json.loads((ROOT / "packaging/android/runtime.json").read_text())
        sdk = Path(sdk)
        tools = sdk / "build-tools" / pins["build_tools"]
        android_jar = sdk / "platforms" / f'android-{pins["target_sdk"]}' / "android.jar"
        if not (tools / "aapt2").is_file() or not android_jar.is_file():
            self.skipTest("missing fixture: pinned Android SDK platform and build-tools")
        if not (shutil.which("rsvg-convert") or shutil.which("magick")):
            self.skipTest("missing fixture: librsvg or ImageMagick icon renderer")
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            assets = output / "assets"
            assets.mkdir()
            (assets / "fixture.txt").write_text("preserved APK asset")
            result = subprocess.run([
                "bash", "-euc", '''
repo_root="$1"
build_tools="$2"
android_jar="$3"
out="$4"
pin() { python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))[sys.argv[2]])' "$repo_root/packaging/android/runtime.json" "$1"; }
min_sdk="$(pin min_sdk)"
client_library=fixture_client
product=Fixture
version_code=1
version=0.0.1
source "$repo_root/packaging/android/render-template.sh"
render "$repo_root/packaging/android/AndroidManifest.xml" "$out/AndroidManifest.xml"
source "$repo_root/packaging/android/resources.sh"
link_apk_resources "$out/AndroidManifest.xml" "$out/assets" "$out"
''', "android-resource-test", str(ROOT), str(tools), str(android_jar), str(output),
            ], text=True, capture_output=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            apk = output / "unsigned.apk"
            badging = subprocess.check_output([str(tools / "aapt2"), "dump", "badging", str(apk)], text=True)
            application = re.search(r"(?m)^application: .* icon='([^']+)'", badging)
            self.assertIsNotNone(application, "application has no launcher icon")
            self.assertIn("launchable-activity:", badging)
            self.assertRegex(badging, r"application-icon-\d+:'[^']+'")
            with zipfile.ZipFile(apk) as archive:
                icon = archive.read(application[1])
                self.assertEqual(icon[:8], b"\x89PNG\r\n\x1a\n")
                width, height = struct.unpack(">II", icon[16:24])
                self.assertEqual(width, height)
                self.assertGreater(width, 1)
                self.assertEqual(archive.read("assets/fixture.txt"), b"preserved APK asset")


if __name__ == "__main__":
    unittest.main()
