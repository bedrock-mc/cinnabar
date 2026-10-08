"""Hermetic release preparation tests: real local Git, fake GitHub responses.

Run: python3 -m unittest discover -s packaging/tests -v
"""

import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import tomllib
import unittest


PACKAGING = Path(__file__).resolve().parents[1]
SOURCE_VERSION = "1.2.3"
MANIFEST = f'''[workspace]
members = ["app", "crates/core", "tools/standalone", "vendor/thing"]

[workspace.package]
version = "{SOURCE_VERSION}" # release version
edition = "2024"

[workspace.dependencies]
external = "{SOURCE_VERSION}"
'''
LOCK = f'''# Keep the lockfile's formatting and dependency pins.
version = 4

[[package]]
name = "app"
version = "{SOURCE_VERSION}"
dependencies = [
 "core {SOURCE_VERSION}",
 "core {SOURCE_VERSION} (registry+https://example.invalid/index)",
 "vendored",
]

[[package]]
name = "core"
version = "{SOURCE_VERSION}"

[[package]]
name = "core"
version = "{SOURCE_VERSION}"
source = "registry+https://example.invalid/index"
checksum = "unchanged-checksum"

[[package]]
name = "standalone"
version = "0.4.0"

[[package]]
name = "vendored"
version = "{SOURCE_VERSION}"
'''


def fixture(root: Path) -> None:
    root.mkdir(parents=True)
    (root / "Cargo.toml").write_text(MANIFEST)
    (root / "Cargo.lock").write_text(LOCK)
    for path, name, declaration in [
        ("app", "app", "version.workspace = true"),
        ("crates/core", "core", "version.workspace = true"),
        ("tools/standalone", "standalone", 'version = "0.4.0"'),
        ("vendor/thing", "vendored", f'version = "{SOURCE_VERSION}"'),
    ]:
        directory = root / path
        directory.mkdir(parents=True)
        (directory / "Cargo.toml").write_text(f'[package]\nname = "{name}"\n{declaration}\n')
    (root / "packaging").mkdir()
    for name in ["bump-version.py", "prepare-release.py"]:
        shutil.copyfile(PACKAGING / name, root / "packaging" / name)


def run(*args: str, cwd: Path, env: dict | None = None, check: bool = True):
    return subprocess.run(args, cwd=cwd, env=env, text=True, capture_output=True, check=check)


class VersionTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name) / "workspace"
        fixture(self.root)

    def bump(self, kind, *args, check=True):
        return run(sys.executable, "packaging/bump-version.py", kind, *args, cwd=self.root, check=check)

    def test_current_and_dry_run_leave_all_files_unchanged(self):
        original = {path: path.read_bytes() for path in self.root.rglob("Cargo.*")}
        self.assertEqual(self.bump("current").stdout.strip(), SOURCE_VERSION)
        self.assertEqual(self.bump("major", "--dry-run").stdout.strip(), "2.0.0")
        self.assertEqual(original, {path: path.read_bytes() for path in original})

    def test_bumps_only_workspace_versions_and_qualified_local_references(self):
        for kind, expected in [("patch", "1.2.4"), ("minor", "1.3.0"), ("major", "2.0.0")]:
            with self.subTest(kind=kind):
                (self.root / "Cargo.toml").write_text(MANIFEST)
                (self.root / "Cargo.lock").write_text(LOCK)
                self.assertEqual(self.bump(kind).stdout.strip(), expected)
                manifest = tomllib.loads((self.root / "Cargo.toml").read_text())
                self.assertEqual(manifest["workspace"]["package"]["version"], expected)
                self.assertEqual(manifest["workspace"]["dependencies"]["external"], SOURCE_VERSION)
                packages = tomllib.loads((self.root / "Cargo.lock").read_text())["package"]
                local = {p["name"]: p for p in packages if "source" not in p}
                self.assertEqual(local["app"]["version"], expected)
                self.assertEqual(local["core"]["version"], expected)
                self.assertEqual(local["app"]["dependencies"][0], f"core {expected}")
                self.assertEqual(local["app"]["dependencies"][1], f"core {SOURCE_VERSION} (registry+https://example.invalid/index)")
                self.assertEqual(local["standalone"]["version"], "0.4.0")
                self.assertEqual(local["vendored"]["version"], SOURCE_VERSION)
                self.assertEqual(next(p for p in packages if "source" in p)["version"], SOURCE_VERSION)
                self.assertIn(f'version = "{expected}" # release version', (self.root / "Cargo.toml").read_text())
                self.assertIn("unchanged-checksum", (self.root / "Cargo.lock").read_text())

    def test_stale_or_missing_lock_entry_fails_before_writing(self):
        for broken in [
            LOCK.replace(f'name = "app"\nversion = "{SOURCE_VERSION}"', 'name = "app"\nversion = "0.0.0"'),
            LOCK.replace('name = "app"', 'name = "missing-app"'),
        ]:
            with self.subTest(lock=broken):
                (self.root / "Cargo.lock").write_text(broken)
                result = self.bump("patch", check=False)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual((self.root / "Cargo.toml").read_text(), MANIFEST)
                self.assertEqual((self.root / "Cargo.lock").read_text(), broken)

    def test_prerelease_and_leading_zero_versions_are_rejected(self):
        for value in ["1.2.3-beta", "01.2.3"]:
            (self.root / "Cargo.toml").write_text(MANIFEST.replace(SOURCE_VERSION, value))
            self.assertNotEqual(self.bump("current", check=False).returncode, 0)

    def test_custom_version_and_invalid_choices(self):
        original = (self.root / "Cargo.toml").read_text()
        for value in ["", "1.2.3", "0.9.9", "01.3.0", "2.0.0-beta", "2.0.0\n"]:
            result = self.bump("custom", "--version", value, check=False)
            self.assertNotEqual(result.returncode, 0, value)
            self.assertEqual((self.root / "Cargo.toml").read_text(), original)
        self.assertEqual(self.bump("custom", "--version", "3.4.5").stdout.strip(), "3.4.5")


class PrepareTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)
        self.root = self.directory / "workspace"
        self.remote = self.directory / "origin.git"
        fixture(self.root)
        run("git", "init", "--bare", "--initial-branch=main", str(self.remote), cwd=self.directory)
        self.git("init", "--initial-branch=main")
        self.git("config", "user.name", "Fixture")
        self.git("config", "user.email", "fixture@example.invalid")
        self.git("add", ".")
        self.git("commit", "-m", "Initial fixture")
        self.git("remote", "add", "origin", str(self.remote))
        self.git("push", "--set-upstream", "origin", "main")
        self.initial = self.git("rev-parse", "HEAD")
        binary = self.directory / "bin"
        binary.mkdir()
        gh = binary / ("gh.py" if os.name == "nt" else "gh")
        gh.write_text('''#!/usr/bin/env python3
import os, sys
from pathlib import Path
tag = sys.argv[-1].rsplit("/", 1)[-1]
with open(os.environ["FAKE_GH_LOG"], "a") as log:
    log.write(tag + "\\n")
error = os.environ.get("FAKE_GH_ERROR")
if error:
    print("HTTP/2.0 " + error + " Error")
    print("GitHub unavailable", file=sys.stderr)
    raise SystemExit(1)
if tag in os.environ.get("FAKE_RELEASES", "").split(","):
    print("HTTP/2.0 200 OK\\n\\n{}")
else:
    print("HTTP/2.0 404 Not Found\\n\\n{}")
    raise SystemExit(1)
''')
        gh.chmod(0o755)
        if os.name == "nt":
            (binary / "gh.cmd").write_text(f'@"{sys.executable}" "{gh}" %*\n')
        self.output = self.directory / "github-output"
        self.log = self.directory / "github-log"
        self.env = dict(os.environ, PATH=str(binary) + os.pathsep + os.environ["PATH"],
                        EVENT_NAME="workflow_dispatch", DEFAULT_BRANCH="main", BUMP="current",
                        GITHUB_REF="refs/heads/main", GITHUB_SHA=self.initial,
                        GITHUB_REPOSITORY="fixture/cinnabar", GITHUB_OUTPUT=str(self.output),
                        CINNABAR_SOURCE_ROOT=str(self.root),
                        FAKE_GH_LOG=str(self.log), FAKE_RELEASES="", FAKE_GH_ERROR="")
        self.env.pop("RELEASE_TAG", None)

    def git(self, *args):
        return run("git", *args, cwd=self.root).stdout.strip()

    def prepare(self, check=True, **changes):
        self.output.unlink(missing_ok=True)
        env = dict(self.env, **changes)
        return run(sys.executable, "packaging/prepare-release.py", cwd=self.root, env=env, check=check)

    def outputs(self):
        return dict(line.split("=", 1) for line in self.output.read_text().splitlines())

    def refs(self):
        return self.git("ls-remote", "origin")

    def assert_no_outputs(self):
        self.assertFalse(self.output.exists())

    def test_main_push_is_read_only_nightly_at_the_event_commit(self):
        before = self.refs()
        self.prepare(EVENT_NAME="push")
        self.assertEqual(self.outputs(), {"channel": "nightly", "tag": "nightly", "version": SOURCE_VERSION,
                                         "ref": self.initial, "commit": self.initial})
        self.assertEqual(self.refs(), before)
        self.assertFalse(self.log.exists())

    def test_scheduled_run_is_read_only_nightly_at_the_default_branch(self):
        before = self.refs()
        self.prepare(EVENT_NAME="schedule")
        self.assertEqual(self.outputs(), {"channel": "nightly", "tag": "nightly", "version": SOURCE_VERSION,
                                         "ref": self.initial, "commit": self.initial})
        self.assertEqual(self.refs(), before)

    def test_current_creates_one_annotated_tag_and_no_version_commit(self):
        self.prepare()
        self.assertEqual(self.git("rev-parse", "HEAD"), self.initial)
        self.assertEqual(self.git("cat-file", "-t", "v" + SOURCE_VERSION), "tag")
        self.assertEqual(self.outputs()["commit"], self.initial)
        self.assertEqual(self.outputs()["tag"], "v" + SOURCE_VERSION)
        self.assertIn("refs/tags/v" + SOURCE_VERSION, self.refs())

    def test_patch_commits_and_atomically_pushes_version_and_tag(self):
        self.prepare(BUMP="patch")
        current = self.git("rev-parse", "HEAD")
        self.assertNotEqual(current, self.initial)
        self.assertEqual(self.outputs()["version"], "1.2.4")
        self.assertEqual(self.outputs()["ref"], current)
        self.assertEqual(self.git("rev-parse", "v1.2.4^{commit}"), current)
        self.assertIn(current + "\trefs/heads/main", self.refs())
        self.assertIn(current + "\trefs/tags/v1.2.4^{}", self.refs())
        self.assertEqual(self.git("log", "-1", "--format=%s"), "chore: release v1.2.4")

    def test_retry_bump_recovers_unpublished_head_tag_without_another_bump(self):
        self.prepare(BUMP="patch")
        current = self.git("rev-parse", "HEAD")
        before = self.refs()
        self.prepare(BUMP="minor")
        self.assertEqual(self.outputs()["version"], "1.2.4")
        self.assertEqual(self.git("rev-parse", "HEAD"), current)
        self.assertEqual(self.refs(), before)

    def test_selected_branch_receives_release_instead_of_main(self):
        self.git("checkout", "-b", "release/candidate")
        self.git("push", "origin", "release/candidate")
        self.prepare(BUMP="minor", TARGET_BRANCH="release/candidate")
        current = self.git("rev-parse", "HEAD")
        self.assertIn(current + "\trefs/heads/release/candidate", self.refs())
        self.assertIn(self.initial + "\trefs/heads/main", self.refs())
        self.assertEqual(self.outputs()["version"], "1.3.0")

    def test_control_tools_release_old_branch_without_packaging_scripts(self):
        tooling = self.directory / "release-tools"
        shutil.copytree(self.root / "packaging", tooling)
        self.git("checkout", "-b", "old/branch")
        self.git("rm", "-r", "packaging")
        self.git("commit", "-m", "Old source without release tooling")
        self.git("push", "origin", "old/branch")
        env = dict(self.env, TARGET_BRANCH="old/branch", BUMP="custom", CUSTOM_VERSION="4.5.6")
        run(sys.executable, str(tooling / "prepare-release.py"), cwd=self.root, env=env)
        self.assertEqual(self.outputs()["version"], "4.5.6")
        self.assertFalse((self.root / "packaging").exists())
        self.assertEqual(self.git("status", "--porcelain"), "")
        self.assertIn(self.initial + "\trefs/heads/main", self.refs())

    def test_custom_release_recovers_exact_unpublished_version(self):
        self.prepare(BUMP="custom", CUSTOM_VERSION="3.4.5")
        current = self.git("rev-parse", "HEAD")
        before = self.refs()
        self.prepare(BUMP="custom", CUSTOM_VERSION="3.4.5")
        self.assertEqual(self.outputs()["version"], "3.4.5")
        self.assertEqual(self.git("rev-parse", "HEAD"), current)
        self.assertEqual(self.refs(), before)

    def test_invalid_branch_and_custom_version_do_not_mutate(self):
        before = self.refs()
        for changes in [{"TARGET_BRANCH": "bad..branch"},
                        {"TARGET_BRANCH": "another"},
                        {"BUMP": "custom", "CUSTOM_VERSION": "2.0.0-beta"}]:
            result = self.prepare(check=False, **changes)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(self.git("rev-parse", "HEAD"), self.initial)
            self.assertEqual(self.refs(), before)
            self.assert_no_outputs()

    def test_existing_current_release_is_rejected_before_mutation(self):
        before = self.refs()
        result = self.prepare(check=False, FAKE_RELEASES="v" + SOURCE_VERSION)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("already exists", result.stderr)
        self.assertEqual(self.refs(), before)
        self.assertEqual(self.git("tag"), "")
        self.assert_no_outputs()

    def test_published_current_release_can_be_followed_by_a_patch(self):
        self.git("tag", "v" + SOURCE_VERSION)
        self.git("push", "origin", "v" + SOURCE_VERSION)
        self.prepare(BUMP="patch", FAKE_RELEASES="v" + SOURCE_VERSION)
        self.assertEqual(self.outputs()["version"], "1.2.4")

    def test_current_tag_on_another_commit_is_rejected(self):
        self.git("tag", "v" + SOURCE_VERSION)
        (self.root / "change.txt").write_text("later commit")
        self.git("add", "change.txt")
        self.git("commit", "-m", "Later commit")
        before = self.refs()
        result = self.prepare(check=False)
        self.assertIn("another commit", result.stderr)
        self.assertEqual(self.refs(), before)
        self.assert_no_outputs()

    def test_conflicting_next_tag_is_rejected_without_a_bump(self):
        self.git("tag", "v1.2.4")
        result = self.prepare(check=False, BUMP="patch")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual((self.root / "Cargo.toml").read_text(), MANIFEST)
        self.assertEqual(self.git("rev-parse", "HEAD"), self.initial)
        self.assert_no_outputs()

    def test_github_authentication_failure_is_not_a_missing_release(self):
        before = self.refs()
        result = self.prepare(check=False, FAKE_GH_ERROR="403")
        self.assertIn("cannot verify", result.stderr)
        self.assertEqual(self.refs(), before)
        self.assertEqual(self.git("tag"), "")
        self.assert_no_outputs()

    def test_dirty_or_nondefault_checkout_is_rejected(self):
        (self.root / "dirty.txt").write_text("uncommitted")
        self.assertNotEqual(self.prepare(check=False).returncode, 0)
        self.assert_no_outputs()
        (self.root / "dirty.txt").unlink()
        self.git("checkout", "-b", "feature")
        self.assertNotEqual(self.prepare(check=False).returncode, 0)
        self.assert_no_outputs()

    def test_tag_push_validates_source_and_does_not_write_remote(self):
        self.git("tag", "v" + SOURCE_VERSION)
        self.git("push", "origin", "v" + SOURCE_VERSION)
        before = self.refs()
        self.prepare(EVENT_NAME="push", GITHUB_REF="refs/tags/v" + SOURCE_VERSION)
        self.assertEqual(self.outputs()["channel"], "stable")
        self.assertEqual(self.outputs()["ref"], self.initial)
        self.assertEqual(self.refs(), before)
        self.assertFalse(self.log.exists())
        for tag in ["v9.9.9", "v1.2.3-beta"]:
            self.git("tag", tag)
            result = self.prepare(check=False, EVENT_NAME="push", GITHUB_REF="refs/tags/" + tag)
            self.assertNotEqual(result.returncode, 0)
            self.assert_no_outputs()

    def test_push_checkout_must_match_event_sha(self):
        (self.root / "change.txt").write_text("later commit")
        self.git("add", "change.txt")
        self.git("commit", "-m", "Later commit")
        result = self.prepare(check=False, EVENT_NAME="push")
        self.assertIn("pushed commit", result.stderr)
        self.assert_no_outputs()

    def test_rejected_branch_push_does_not_leave_a_remote_release_tag(self):
        other = self.directory / "other"
        run("git", "clone", str(self.remote), str(other), cwd=self.directory)
        run("git", "config", "user.name", "Other", cwd=other)
        run("git", "config", "user.email", "other@example.invalid", cwd=other)
        (other / "race.txt").write_text("branch advanced during preparation")
        run("git", "add", "race.txt", cwd=other)
        run("git", "commit", "-m", "Concurrent branch advance", cwd=other)
        run("git", "push", "origin", "main", cwd=other)
        before = self.refs()
        result = self.prepare(check=False, BUMP="patch")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.refs(), before)
        self.assertNotIn("refs/tags/v1.2.4", self.refs())
        self.assert_no_outputs()


if __name__ == "__main__":
    unittest.main()
