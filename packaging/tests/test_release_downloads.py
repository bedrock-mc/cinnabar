"""Release download contracts, exercised through the published helper CLI."""

import hashlib
import json
from pathlib import Path
import re
import shutil
import stat
import subprocess
import sys
import tempfile
import unittest


PACKAGING = Path(__file__).resolve().parents[1]


class ReleaseDownloadsTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.tools = self.root / "tools"
        self.tools.mkdir()
        for filename in ("release-downloads.py", "install.sh.in"):
            shutil.copyfile(PACKAGING / filename, self.tools / filename)
        self.config = json.loads((PACKAGING / "release-assets.json").read_text())
        # Alter the fixture to prove consumers read the manifest rather than literals.
        self.config["repository"] = "fixture/download-contract"
        self.config["checksums"] = "fixture-" + self.config["checksums"]
        self.config["install_script"] = "fixture-" + self.config["install_script"]
        self.config["update_manifest"] = "fixture-" + self.config["update_manifest"]
        self.config["assets"] = {
            platform: {arch: "fixture-" + name for arch, name in arches.items()}
            for platform, arches in self.config["assets"].items()
        }
        self.config["additional_assets"] = [
            "fixture-" + name for name in self.config["additional_assets"]
        ]
        (self.tools / "release-assets.json").write_text(json.dumps(self.config))
        self.dist = self.root / "dist"
        self.dist.mkdir()

    def run_helper(self, *args):
        return subprocess.run(
            [sys.executable, str(self.tools / "release-downloads.py"), *map(str, args)],
            text=True, capture_output=True,
        )

    def required_assets(self):
        return [
            name for arches in self.config["assets"].values() for name in arches.values()
        ] + self.config["additional_assets"] + [self.config["install_script"]]

    def populate(self):
        for name in self.required_assets():
            (self.dist / name).write_bytes(name.encode() + b"\x00release payload\xff")

    def test_every_required_asset_must_exist_before_checksum_is_written(self):
        self.populate()
        for name in self.required_assets():
            with self.subTest(asset=name):
                path = self.dist / name
                payload = path.read_bytes()
                path.unlink()
                result = self.run_helper("checksums", self.dist)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(name, result.stderr)
                self.assertFalse((self.dist / self.config["checksums"]).exists())
                path.write_bytes(payload)

    def test_a_directory_does_not_satisfy_a_required_asset(self):
        self.populate()
        path = self.dist / self.config["install_script"]
        path.unlink()
        path.mkdir()
        result = self.run_helper("checksums", self.dist)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(path.name, result.stderr)
        self.assertFalse((self.dist / self.config["checksums"]).exists())

    def test_final_checksum_includes_metadata_and_is_deterministic_on_rerun(self):
        self.populate()
        (self.dist / self.config["update_manifest"]).write_text('{"signed": "fixture"}\n')
        (self.dist / "extra-artifact.fixture").write_bytes(b"extra artifact")
        (self.dist / "artifact-directory").mkdir()
        checksum = self.dist / self.config["checksums"]
        checksum.write_text("stale checksum content\n")
        result = self.run_helper("checksums", self.dist)
        self.assertEqual(result.returncode, 0, result.stderr)
        first = checksum.read_bytes()
        entries = [line.split("  ", 1) for line in first.decode().splitlines()]
        files = {path.name: path for path in self.dist.iterdir()
                 if path.is_file() and path != checksum}
        self.assertEqual([name for _, name in entries], sorted(files))
        for digest, name in entries:
            self.assertEqual(digest, hashlib.sha256(files[name].read_bytes()).hexdigest())
        result = self.run_helper("checksums", self.dist)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(checksum.read_bytes(), first)

    def test_asset_commands_follow_the_manifest(self):
        for platform, arches in self.config["assets"].items():
            for arch, name in arches.items():
                with self.subTest(platform=platform, arch=arch):
                    result = self.run_helper("asset", platform, arch)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(result.stdout.strip(), name)

    def test_installer_is_standalone_executable_and_uses_manifest_values(self):
        output = self.root / "nested" / self.config["install_script"]
        result = self.run_helper("install-script", "--output", output)
        self.assertEqual(result.returncode, 0, result.stderr)
        script = output.read_text()
        self.assertIsNone(re.search(r"@[A-Z_0-9]+@", script))
        for value in (self.config["repository"], self.config["checksums"],
                      self.config["assets"]["linux"]["x86_64"]):
            self.assertIn(value, script)
        self.assertTrue(output.stat().st_mode & stat.S_IXUSR)
        result = subprocess.run(["sh", str(output), "--help"], text=True, capture_output=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("Usage:", result.stdout)

    def test_workflow_environment_filenames_follow_the_manifest(self):
        common = {
            "INSTALL_SCRIPT": self.config["install_script"],
            "UPDATE_MANIFEST": self.config["update_manifest"],
            "CHECKSUM_ASSET": self.config["checksums"],
        }
        result = self.run_helper("env")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(dict(line.split("=", 1) for line in result.stdout.splitlines()), common)
        for platform, arches in self.config["assets"].items():
            for arch, name in arches.items():
                with self.subTest(platform=platform, arch=arch):
                    result = self.run_helper("env", platform, arch)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    expected = dict(common, RELEASE_ASSET=name)
                    if platform == "windows":
                        expected["AUXILIARY_ASSET"] = self.config["additional_assets"][0]
                    self.assertEqual(
                        dict(line.split("=", 1) for line in result.stdout.splitlines()), expected,
                    )

    def test_updater_specs_pair_download_urls_with_the_same_local_assets(self):
        base = "https://download-contract.invalid/releases/test-version"
        directory = self.root / "artifact directory with spaces"
        result = self.run_helper("update-artifacts", base, directory)
        self.assertEqual(result.returncode, 0, result.stderr)
        specs = [line.split("=", 2) for line in result.stdout.splitlines()]
        expected = {
            f"{platform}-{arch}": (base + "/" + name, str(directory / name))
            for platform, arches in self.config["assets"].items()
            for arch, name in arches.items()
        }
        self.assertEqual(len(specs), len(expected))
        self.assertEqual({platform: (url, local) for platform, url, local in specs}, expected)

    def test_windows_job_retains_the_workflow_source_working_directory(self):
        workflow = (PACKAGING.parent / ".github/workflows/package.yml").read_text()
        global_defaults = re.search(r"(?m)^defaults:\n((?:^  .*\n)+)", workflow)
        self.assertIsNotNone(global_defaults, "workflow run defaults are missing")
        source = re.search(r"(?m)^    working-directory:\s*(.+)$", global_defaults[1])
        self.assertIsNotNone(source, "workflow source directory is missing")
        windows = re.search(
            r"(?ms)^  windows:\n(.*?)(?=^  [A-Za-z_][A-Za-z_0-9-]*:\n|\Z)", workflow,
        )
        self.assertIsNotNone(windows, "Windows packaging job is missing")
        defaults = re.search(r"(?m)^    defaults:\n((?:^      .*\n)+)", windows[1])
        self.assertIsNotNone(defaults, "Windows run defaults are missing")
        working_directory = re.search(r"(?m)^        working-directory:\s*(.+)$", defaults[1])
        self.assertIsNotNone(
            working_directory,
            "Windows job overrides run defaults, so it must retain the source directory",
        )
        self.assertEqual(working_directory[1], source[1])


if __name__ == "__main__":
    unittest.main()
