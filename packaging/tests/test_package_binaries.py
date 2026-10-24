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
    def test_shaderpack_assets_build_the_bounded_compiler_host(self):
        """The opted-in developer path builds the compiler mode used by the client."""
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
            for enabled in (0, 1):
                log.write_text("")
                subprocess.run(
                    ["make", "--no-print-directory", "assets", f"SHADERPACKS={enabled}", f"CARGO={command}"],
                    cwd=ROOT, env={**os.environ, "PACKAGE_TEST_LOG": str(log)},
                    capture_output=True, text=True, check=True,
                )
                calls = [json.loads(line) for line in log.read_text().splitlines()]
                self.assertEqual(len(calls), 1)
                self.assertEqual(calls[0][calls[0].index("--bin") + 1], "assetc")
                self.assertEqual("--features" in calls[0], bool(enabled))
                if enabled:
                    self.assertEqual(calls[0][calls[0].index("--features") + 1], "shaderpacks")

    def test_package_build_uses_the_requested_channel_features(self):
        """The same build enables the selected features for the client and compiler."""
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
            metadata = json.loads((ROOT / "packaging/release-assets.json").read_text())
            for release in metadata["update_channels"].values():
                features = ",".join(release.get("cargo_features", []))
                log.write_text("")
                subprocess.run(
                    ["make", "--no-print-directory", "package-binaries",
                     f"PACKAGE_CARGO_FEATURES={features}", f"CARGO={command} cargo", f"GO={command} go"],
                    cwd=ROOT, env={**os.environ, "PACKAGE_TEST_LOG": str(log)},
                    capture_output=True, text=True, check=True,
                )
                calls = [json.loads(line) for line in log.read_text().splitlines()]
                args = next(args for tool, *args in calls if tool == "cargo")
                self.assertEqual("--features" in args, bool(features))
                if features:
                    self.assertEqual(args[args.index("--features") + 1], features)
                packages = [args[index + 1] for index, arg in enumerate(args) if arg == "-p"]
                self.assertIn("cinnabar", packages)
                self.assertIn("asset-compiler", packages)

    def test_runtime_table_has_unique_names_packages_and_overrides(self):
        """All consumers receive complete, unambiguous runtime binary records."""
        rows = [line.split() for line in (ROOT / "packaging/common/runtime-binaries.txt").read_text().splitlines()]
        self.assertTrue(rows)
        for row in rows:
            self.assertEqual(len(row), 3)
        self.assertEqual(len({row[0] for row in rows}), len(rows))
        self.assertEqual(len({row[2] for row in rows}), len(rows))
        for name, package, override in rows:
            self.assertRegex(name, r"^[a-z0-9-]+$")
            self.assertRegex(package, r"^[a-z0-9-]+$")
            self.assertRegex(override, r"^[A-Z_]+$")

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
                    self.assertNotIn("--features", args)
                    self.assertIn("--release", args)
                    self.assertIn("--no-default-features", args)
                    runtime = [line.split() for line in (ROOT / "packaging/common/runtime-binaries.txt").read_text().splitlines()]
                    packages = [args[index + 1] for index, arg in enumerate(args) if arg == "-p"]
                    binaries = [args[index + 1] for index, arg in enumerate(args) if arg == "--bin"]
                    self.assertIn("cinnabar-cxb", binaries)
                    for name, package, _ in runtime:
                        if package != "-":
                            self.assertIn(package, packages)
                            self.assertIn(name, binaries)
                    go_outputs = {
                        Path(args[args.index("-o") + 1]).name.removesuffix(".exe")
                        for tool, *args in calls if tool == "go" and "-o" in args
                    }
                    self.assertEqual(go_outputs, {name for name, package, _ in runtime if package == "-"})


if __name__ == "__main__":
    unittest.main()
