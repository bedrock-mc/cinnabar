"""Exercise the release APK signing step without building the Android client."""

import base64
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
ASSET = json.loads((ROOT / "packaging/release-assets.json").read_text())["manual_assets"]["android"]["arm64"]
SIGNING_SECRETS = (
    "CINNABAR_ANDROID_KEYSTORE_BASE64", "CINNABAR_ANDROID_KEY_ALIAS",
    "CINNABAR_ANDROID_STORE_PASSWORD", "CINNABAR_ANDROID_KEY_PASSWORD",
)


def signing_step():
    """Read the executable signing step so the fixture exercises the published workflow."""
    workflow = (ROOT / ".github/workflows/package.yml").read_text()
    block = workflow.split("      - name: Build and sign the release APK\n", 1)[1]
    script = block.split("        run: |\n", 1)[1].split("\n      - ", 1)[0]
    return "\n".join(line[10:] for line in script.splitlines())


class AndroidReleaseTests(unittest.TestCase):
    def setUp(self):
        """Use a fake APK builder that checks the imported key and records its temporary path."""
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.runner_temp = self.root / "runner temp"
        self.runner_temp.mkdir()
        builder = self.root / "packaging/android/build-apk.sh"
        builder.parent.mkdir(parents=True)
        builder.write_text('''set -eu
test "$(cat "$CINNABAR_ANDROID_KEYSTORE")" = fixture-keystore
test "$(stat -c %a "$CINNABAR_ANDROID_KEYSTORE")" = 600
test "$CINNABAR_ANDROID_KEY_ALIAS" = fixture-alias
test "$CINNABAR_ANDROID_STORE_PASSWORD" = fixture-store-password
test "$CINNABAR_ANDROID_KEY_PASSWORD" = fixture-key-password
printf '%s' "$CINNABAR_ANDROID_KEYSTORE" > key-path
[[ "${FAIL_BUILD:-}" != true ]] || exit 42
mkdir -p target/android
printf fixture-apk > target/android/fixture-1.2.3-arm64-v8a.apk
''')
        self.env = dict(os.environ, RUNNER_TEMP=str(self.runner_temp), RELEASE_ASSET=ASSET,
                        CINNABAR_ANDROID_KEYSTORE_BASE64=base64.b64encode(b"fixture-keystore").decode(),
                        CINNABAR_ANDROID_KEY_ALIAS="fixture-alias",
                        CINNABAR_ANDROID_STORE_PASSWORD="fixture-store-password",
                        CINNABAR_ANDROID_KEY_PASSWORD="fixture-key-password")

    def run_step(self):
        """Run the workflow shell with the runner's failure handling."""
        return subprocess.run(["bash", "-euo", "pipefail", "-c", signing_step()],
                              cwd=self.root, env=self.env, capture_output=True, text=True)

    def assert_key_removed(self):
        """The decoded keystore must not survive success or a failed build."""
        key = Path((self.root / "key-path").read_text())
        self.assertFalse(key.exists())
        self.assertFalse(list(self.runner_temp.glob("*.keystore")))

    def test_signed_apk_uses_the_manifest_name_and_removes_the_key(self):
        result = self.run_step()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((self.runner_temp / ASSET).read_bytes(), b"fixture-apk")
        self.assert_key_removed()

    def test_a_failed_build_removes_the_key_and_publishes_no_apk(self):
        self.env["FAIL_BUILD"] = "true"
        result = self.run_step()
        self.assertEqual(result.returncode, 42, result.stderr)
        self.assertFalse((self.runner_temp / ASSET).exists())
        self.assert_key_removed()

    def test_missing_signing_secrets_never_fall_back_to_a_test_key(self):
        for secret in SIGNING_SECRETS:
            with self.subTest(secret=secret):
                saved = self.env.pop(secret)
                result = self.run_step()
                self.env[secret] = saved
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(secret, result.stderr)
                self.assertFalse((self.root / "key-path").exists())
                self.assertFalse(list(self.runner_temp.glob("*.keystore")))

    def test_an_invalid_keystore_encoding_is_removed_without_building(self):
        self.env["CINNABAR_ANDROID_KEYSTORE_BASE64"] = "invalid base64!"
        result = self.run_step()
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((self.root / "key-path").exists())
        self.assertFalse(list(self.runner_temp.glob("*.keystore")))


if __name__ == "__main__":
    unittest.main()
