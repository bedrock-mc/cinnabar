"""Exercise release build commands without compiling native binaries."""

import json
import os
import plistlib
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]


@unittest.skipUnless(shutil.which("make"), "GNU make is required")
class PackageBinariesTests(unittest.TestCase):
    def test_macos_bundle_declares_game_mode(self):
        template = (ROOT / "packaging/macos/Info.plist.in").read_text()
        rendered = template.replace("@BUNDLE_ID@", "app.cinnabar.test").replace("@VERSION@", "1.0.0")
        plist = plistlib.loads(rendered.encode())
        self.assertIs(plist.get("LSSupportsGameMode"), True)
        self.assertEqual(plist["LSApplicationCategoryType"], "public.app-category.games")

    def test_play_keeps_default_features_with_release_and_tracy(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            log = root / "commands.jsonl"
            stub = root / "build_stub.py"
            stub.write_text(
                "import json, os, sys\n"
                "with open(os.environ['PACKAGE_TEST_LOG'], 'a') as log:\n"
                "    log.write(json.dumps(sys.argv[1:]) + '\\n')\n"
            )
            command = f"{shlex.quote(sys.executable)} {shlex.quote(str(stub))}"
            for profile in ("play", "release"):
                for tracy in (0, 1):
                    with self.subTest(profile=profile, tracy=tracy):
                        log.write_text("")
                        subprocess.run(
                            ["make", "--no-print-directory", "-o", "assets", "-o", "physics-assets",
                             "play", f"PROFILE={profile}", f"TRACY={tracy}",
                             f"CARGO={command} cargo", f"GO={command} go"],
                            cwd=ROOT, env=dict(os.environ, PACKAGE_TEST_LOG=str(log)),
                            capture_output=True, text=True, check=True,
                        )
                        calls = [json.loads(line) for line in log.read_text().splitlines()]
                        args = next(args for tool, *args in calls if tool == "cargo" and args[0] == "run")
                        self.assertNotIn("--no-default-features", args)
                        self.assertEqual(args[args.index("--profile") + 1], profile)
                        if tracy:
                            self.assertIn("tracy", args)

    def test_release_clients_enable_local_mods_on_every_platform(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            log = root / "commands.jsonl"
            stub = root / "build_stub.py"
            stub.write_text(
                "import json, os, sys\n"
                "with open(os.environ['PACKAGE_TEST_LOG'], 'a') as log:\n"
                "    log.write(json.dumps(sys.argv[1:]) + '\\n')\n"
            )
            command = f"{shlex.quote(sys.executable)} {shlex.quote(str(stub))}"
            env = {**os.environ, "PACKAGE_TEST_LOG": str(log)}
            for platform in ("linux", "windows", "macos"):
                with self.subTest(platform=platform):
                    log.write_text("")
                    subprocess.run(
                        [
                            "make", "--no-print-directory", "package-binaries",
                            f"CARGO={command} cargo", f"GO={command} go",
                            f"DIST_PLATFORM={platform}",
                        ],
                        cwd=ROOT, env=env, capture_output=True, text=True, check=True,
                    )
                    calls = [json.loads(line) for line in log.read_text().splitlines()]
                    builds = [args for tool, *args in calls if tool == "cargo" and args[0] == "build"]
                    self.assertEqual(len(builds), 1)
                    args = builds[0]
                    features = {
                        feature
                        for index, arg in enumerate(args) if arg == "--features"
                        for feature in args[index + 1].replace(",", " ").split()
                    }
                    self.assertIn("bedrock-client/local-mods", features)
                    self.assertIn("--release", args)
                    self.assertIn("--no-default-features", args)


if __name__ == "__main__":
    unittest.main()
