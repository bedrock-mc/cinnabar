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
        self.config["update_payload_prefix"] = "fixture-" + self.config["update_payload_prefix"]
        self.config["update_channels"] = {
            channel: {**release, **({"tag": "fixture-" + release["tag"]} if "tag" in release else {}),
                      "manifest": "fixture-" + release["manifest"]}
            for channel, release in self.config["update_channels"].items()
        }
        self.config["assets"] = {
            platform: {arch: "fixture-" + name for arch, name in arches.items()}
            for platform, arches in self.config["assets"].items()
        }
        self.config["manual_assets"] = {
            platform: {arch: "fixture-" + name for arch, name in arches.items()}
            for platform, arches in self.config["manual_assets"].items()
        }
        self.config["update_assets"] = {
            platform: {arch: "fixture-" + name for arch, name in arches.items()}
            for platform, arches in self.config["update_assets"].items()
        }
        self.config["symbols"] = {**self.config["symbols"],
                                  "archive": "fixture-" + self.config["symbols"]["archive"]}
        self.version = "9.8.7"
        (self.tools / "release-assets.json").write_text(json.dumps(self.config))
        self.dist = self.root / "dist"
        self.dist.mkdir()

    def run_helper(self, *args):
        return subprocess.run(
            [sys.executable, str(self.tools / "release-downloads.py"), *map(str, args)],
            text=True, capture_output=True,
        )

    def symbol_archive(self, platform, arch):
        return self.config["symbols"]["archive"].format(version=self.version, platform=platform, arch=arch)

    def required_assets(self):
        return [
            name for arches in self.config["assets"].values() for name in arches.values()
        ] + [
            name for arches in self.config["manual_assets"].values() for name in arches.values()
        ] + [name for arches in self.config["update_assets"].values() for name in arches.values()] + [
            self.symbol_archive(platform, arch)
            for platform, arches in self.config["assets"].items() for arch in arches
        ] + [self.config["install_script"]]

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
                result = self.run_helper("checksums", self.dist, "--version", self.version)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(name, result.stderr)
                self.assertFalse((self.dist / self.config["checksums"]).exists())
                path.write_bytes(payload)

    def test_a_directory_does_not_satisfy_a_required_asset(self):
        self.populate()
        path = self.dist / self.config["install_script"]
        path.unlink()
        path.mkdir()
        result = self.run_helper("checksums", self.dist, "--version", self.version)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(path.name, result.stderr)
        self.assertFalse((self.dist / self.config["checksums"]).exists())

    def test_final_checksum_includes_metadata_and_is_deterministic_on_rerun(self):
        self.populate()
        (self.dist / "signed-fixture.json").write_text('{"signed": "fixture"}\n')
        (self.dist / "extra-artifact.fixture").write_bytes(b"extra artifact")
        (self.dist / "artifact-directory").mkdir()
        checksum = self.dist / self.config["checksums"]
        checksum.write_text("stale checksum content\n")
        result = self.run_helper("checksums", self.dist, "--version", self.version)
        self.assertEqual(result.returncode, 0, result.stderr)
        first = checksum.read_bytes()
        entries = [line.split("  ", 1) for line in first.decode().splitlines()]
        files = {path.name: path for path in self.dist.iterdir()
                 if path.is_file() and path != checksum}
        self.assertEqual([name for _, name in entries], sorted(files))
        for digest, name in entries:
            self.assertEqual(digest, hashlib.sha256(files[name].read_bytes()).hexdigest())
        result = self.run_helper("checksums", self.dist, "--version", self.version)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(checksum.read_bytes(), first)

    def test_only_a_release_without_split_debug_info_may_omit_symbol_archives(self):
        self.populate()
        archives = [self.symbol_archive(platform, arch)
                    for platform, arches in self.config["assets"].items() for arch in arches]
        for name in archives:
            (self.dist / name).unlink()
        result = self.run_helper("checksums", self.dist, "--version", self.version)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(archives[0], result.stderr)
        result = self.run_helper("checksums", self.dist, "--version", self.version, "--without-symbols")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue((self.dist / self.config["checksums"]).is_file())

    def test_symbol_archives_are_named_by_version_platform_and_arch(self):
        names = {self.symbol_archive(platform, arch)
                 for platform, arches in self.config["assets"].items() for arch in arches}
        self.assertEqual(len(names), sum(len(arches) for arches in self.config["assets"].values()))
        for platform, arches in self.config["assets"].items():
            for arch in arches:
                with self.subTest(platform=platform, arch=arch):
                    name = self.symbol_archive(platform, arch)
                    self.assertIn(self.version, name)
                    self.assertIn(f"{platform}-{arch}", name)

    def test_asset_commands_follow_the_manifest(self):
        for platform, arches in (self.config["assets"] | self.config["manual_assets"]).items():
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
                      *self.config["assets"]["linux"].values()):
            self.assertIn(value, script)
        self.assertTrue(output.stat().st_mode & stat.S_IXUSR)
        result = subprocess.run(["sh", str(output), "--help"], text=True, capture_output=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("Usage:", result.stdout)

    def test_workflow_environment_filenames_follow_the_manifest(self):
        common = {
            "INSTALL_SCRIPT": self.config["install_script"],
            "CHECKSUM_ASSET": self.config["checksums"],
            "UPDATE_PAYLOAD_PREFIX": self.config["update_payload_prefix"],
        }
        result = self.run_helper("env")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(dict(line.split("=", 1) for line in result.stdout.splitlines()), common)
        for platform, arches in self.config["assets"].items():
            for arch, name in arches.items():
                with self.subTest(platform=platform, arch=arch):
                    result = self.run_helper("env", platform, arch, "--version", self.version)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    expected = dict(common, RELEASE_ASSET=name,
                                    SYMBOLS_ASSET=self.symbol_archive(platform, arch))
                    if platform == "windows":
                        expected["AUXILIARY_ASSET"] = self.config["update_assets"][platform][arch]
                    self.assertEqual(
                        dict(line.split("=", 1) for line in result.stdout.splitlines()), expected,
                    )
                    without_version = self.run_helper("env", platform, arch)
                    self.assertNotEqual(without_version.returncode, 0)
                    self.assertNotIn("RELEASE_ASSET=", without_version.stdout)

    def test_each_channel_names_its_manifest_and_release(self):
        for channel, release in self.config["update_channels"].items():
            with self.subTest(channel=channel):
                result = self.run_helper("env", "--channel", channel, "--tag", "v1.2.3")
                self.assertEqual(result.returncode, 0, result.stderr)
                values = dict(line.split("=", 1) for line in result.stdout.splitlines())
                self.assertEqual(values["UPDATE_MANIFEST"], release["manifest"])
                self.assertEqual(values["UPDATE_MANIFEST_TAG"],
                                 "v1.2.3" if release["track"] == "tagged" else release["tag"])

    def test_manual_installers_have_downloads_without_desktop_symbol_archives(self):
        """Manual platforms publish required downloads without joining the desktop symbol pipeline."""
        for platform, arches in self.config["manual_assets"].items():
            for arch, name in arches.items():
                with self.subTest(platform=platform, arch=arch):
                    result = self.run_helper("env", platform, arch, "--version", self.version)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    values = dict(line.split("=", 1) for line in result.stdout.splitlines())
                    self.assertEqual(values["RELEASE_ASSET"], name)
                    self.assertNotIn("SYMBOLS_ASSET", values)
        self.populate()
        result = self.run_helper("checksums", self.dist, "--version", self.version)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_unpublished_stable_and_missing_version_tags_are_rejected(self):
        for args in [("env", "--channel", "stable", "--tag", "v1.2.3"),
                     ("env", "--channel", "beta")]:
            result = self.run_helper(*args)
            self.assertNotEqual(result.returncode, 0)
            self.assertNotIn("UPDATE_MANIFEST=", result.stdout)

    def test_update_payloads_are_content_addressed_copies_of_native_artifacts(self):
        base = "https://download-contract.invalid/releases/test-version"
        directory = self.root / "artifact directory with spaces"
        self.dist = directory
        directory.mkdir()
        self.populate()
        out = self.root / "payloads"
        result = self.run_helper("update-payloads", base, directory, out)
        self.assertEqual(result.returncode, 0, result.stderr)
        specs = [line.split("=", 2) for line in result.stdout.splitlines()]
        expected, copies = {}, []
        for platform, arches in self.config["assets"].items():
            for arch, name in arches.items():
                selected = self.config["update_assets"].get(platform, {}).get(arch, name)
                digest = hashlib.sha256((directory / selected).read_bytes()).hexdigest()
                payload = f'{self.config["update_payload_prefix"]}{digest[:16]}-{selected}'
                expected[f"{platform}-{arch}"] = (base + "/" + payload, str(out / payload))
                # A byte-identical copy, so its name changes whenever its content does.
                copies.append((out / payload, directory / selected))
        self.assertEqual(len(specs), len(expected))
        self.assertEqual({platform: (url, local) for platform, url, local in specs}, expected)
        for copy, source in copies:
            self.assertEqual(copy.read_bytes(), source.read_bytes())

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
