"""Check artifact reuse, changed-input invalidation, and bounded cache retention."""

import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch


MODULE = Path(__file__).resolve().parents[1] / "actions/build-cache/cache.py"
SPEC = importlib.util.spec_from_file_location("build_cache", MODULE)
cache = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(cache)


class SourceReuseTests(unittest.TestCase):
    """Exercise a real Cargo build across simulated fresh source checkouts."""

    def setUp(self):
        """Create an isolated, dependency-free crate with an included data file."""
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / "src").mkdir()
        (self.root / "Cargo.toml").write_text(
            '[package]\nname = "cache-fixture"\nversion = "0.1.0"\nedition = "2021"\n'
            '[profile.release]\nlto = "thin"\ncodegen-units = 1\n'
        )
        (self.root / ".gitignore").write_text('/target\n')
        (self.root / "src/main.rs").write_text(
            'fn main() { print!("{}", include_str!("../value.txt")); }\n'
        )
        (self.root / "value.txt").write_text("before")
        (self.root / "build.rs").write_text(
            'fn main() { println!("cargo:rerun-if-changed=src"); }\n'
        )
        cache.command("git", "init", "-q", cwd=self.root)
        cache.command("cargo", "generate-lockfile", "--offline", cwd=self.root)
        cache.command("git", "add", ".", cwd=self.root)
        cache.restore_inputs(self.root)

    def build(self, release=False):
        """Build the fixture and return whether Cargo reused every compilation."""
        result = subprocess.run(
            ["cargo", "build", "--offline", "--locked", "--message-format=json"]
            + (["--release"] if release else []),
            cwd=self.root, env={**os.environ, "CARGO_INCREMENTAL": "0", "CARGO_TARGET_DIR": str(self.root / "target")},
            capture_output=True, text=True, check=True,
        )
        artifacts = [event for line in result.stdout.splitlines()
                     if (event := json.loads(line))["reason"] == "compiler-artifact"]
        self.assertTrue(artifacts)
        return all(event["fresh"] for event in artifacts)

    def fresh_checkout(self):
        """Give tracked inputs fresh mtimes, as actions/checkout does on a new runner."""
        for name in cache.snapshot(self.root):
            os.utime(self.root / name, None)
        os.utime(self.root / "src", None)

    def test_unchanged_checkout_reuses_compilation(self):
        """Content-checked timestamp restoration prevents checkout-only rebuilds."""
        self.assertFalse(self.build())
        cache.verify_inputs(self.root)
        self.fresh_checkout()
        cache.restore_inputs(self.root)
        self.assertTrue(self.build())

    def test_checkout_without_restoration_rebuilds(self):
        """Demonstrate the regression that timestamp restoration must prevent."""
        self.assertFalse(self.build())
        self.fresh_checkout()
        self.assertFalse(self.build())

    def test_release_checkout_reuses_compilation(self):
        """Release artifacts remain reusable with the shipped LTO/codegen settings."""
        self.assertFalse(self.build(release=True))
        cache.verify_inputs(self.root)
        self.fresh_checkout()
        cache.restore_inputs(self.root)
        self.assertTrue(self.build(release=True))

    def test_new_untracked_directory_input_reruns_build_script(self):
        """A newly discovered input must invalidate a directory-watching build script."""
        self.build()
        (self.root / "src/extra.txt").write_text("new input")
        cache.restore_inputs(self.root)
        self.assertFalse(self.build())

    def test_changed_included_data_rebuilds_and_changes_output(self):
        """Changed non-Rust inputs retain fresh timestamps and invalidate artifacts."""
        self.build()
        self.fresh_checkout()
        (self.root / "value.txt").write_text("after")
        # Simulate checking out before another runner finished the restored build.
        os.utime(self.root / "value.txt", ns=(1_000_000_000, 1_000_000_000))
        cache.restore_inputs(self.root)
        self.assertFalse(self.build())
        executable = self.root / "target/debug" / ("cache-fixture.exe" if os.name == "nt" else "cache-fixture")
        self.assertEqual(cache.command(str(executable)), "after")

    def test_build_cannot_change_inputs_before_save(self):
        """Avoid saving artifacts against a snapshot of different source content."""
        (self.root / "value.txt").write_text("changed during build")
        with self.assertRaisesRegex(RuntimeError, "Tracked sources changed"):
            cache.verify_inputs(self.root)

    def test_key_changes_per_revision_and_separates_compiler_settings(self):
        """New revisions refresh caches without losing the compatible restore prefix."""
        env = {"RUNNER_OS": "Linux", "RUNNER_ARCH": "X64"}
        with patch.object(cache, "command", side_effect=["rustc 1", "first", "rustc 1", "second", "rustc 1", "second"]):
            first = cache.plan(self.root, "rust", "ci", env)
            second = cache.plan(self.root, "rust", "ci", env)
            changed = cache.plan(self.root, "rust", "ci", {**env, "RUSTFLAGS": "-C target-cpu=native"})
        self.assertNotEqual(first["key"], second["key"])
        self.assertEqual(first["prefix"], second["prefix"])
        self.assertNotEqual(second["prefix"], changed["prefix"])


class CrossTargetTests(unittest.TestCase):
    """Keep cross-compiled artifacts and their caches separate on the same runner."""

    def test_rust_targets_cache_both_host_and_target_outputs(self):
        """Cross builds retain native tools and target artifacts in separate buckets."""
        root = Path("source").resolve()
        env = {"RUNNER_OS": "macOS", "RUNNER_ARCH": "ARM64"}
        plans = []
        for target in ["aarch64-apple-darwin", "x86_64-apple-darwin"]:
            with patch.object(cache, "command", side_effect=["rustc 1", "revision"]):
                plan = cache.plan(root, "rust", "release", {**env, "CARGO_BUILD_TARGET": target})
            paths = plan["paths"].splitlines()
            self.assertIn(str(root / "target/release"), paths)
            self.assertIn(str(root / "target" / target / "release"), paths)
            self.assertIn(str(root / "target/.ci-inputs.json"), paths)
            plans.append(plan)
        self.assertNotEqual(plans[0]["bucket"], plans[1]["bucket"])
        self.assertNotEqual(plans[0]["prefix"], plans[1]["prefix"])
        entries = [
            {"id": 1, "key": plans[0]["key"], "ref": "refs/heads/dev", "created_at": "2026-01-01"},
            {"id": 2, "key": plans[1]["key"], "ref": "refs/heads/dev", "created_at": "2026-01-02"},
        ]
        self.assertEqual(cache.superseded(entries, plans[1]["key"], plans[1]["bucket"], "refs/heads/dev"), [])

    def test_go_targets_and_cgo_settings_separate_caches(self):
        """Parallel Go targets cannot collide or reuse incompatible cgo settings."""
        root = Path("source").resolve()
        env = {"RUNNER_OS": "macOS", "RUNNER_ARCH": "ARM64", "GOOS": "darwin", "CGO_ENABLED": "1"}
        plans = []
        for settings in [{"GOARCH": "arm64"}, {"GOARCH": "amd64"}, {"GOARCH": "amd64", "CGO_ENABLED": "0"}]:
            with patch.object(cache, "command", side_effect=["go version 1", "cache\nmodules", "revision"]):
                plans.append(cache.plan(root, "go", "ci", {**env, **settings}))
        self.assertNotEqual(plans[0]["bucket"], plans[1]["bucket"])
        self.assertNotEqual(plans[1]["prefix"], plans[2]["prefix"])
        self.assertEqual(plans[0]["paths"], "cache\nmodules")


class RetentionTests(unittest.TestCase):
    """Protect the replacement, other platforms, and concurrent newer saves."""

    def test_remove_only_superseded_entries_on_the_same_ref(self):
        """A replacement permits removing older entries from only its own bucket."""
        entries = [
            {"id": 1, "key": "linux-old", "ref": "refs/heads/dev", "created_at": "2026-01-01"},
            {"id": 2, "key": "linux-current", "ref": "refs/heads/dev", "created_at": "2026-01-02"},
            {"id": 3, "key": "linux-newer", "ref": "refs/heads/dev", "created_at": "2026-01-03"},
            {"id": 4, "key": "macos-old", "ref": "refs/heads/dev", "created_at": "2026-01-01"},
            {"id": 5, "key": "linux-old", "ref": "refs/pull/1/merge", "created_at": "2026-01-01"},
        ]
        self.assertEqual(cache.superseded(entries, "linux-current", "linux-", "refs/heads/dev"), [1])
        self.assertEqual(cache.superseded(entries, "linux-unsaved", "linux-", "refs/heads/dev"), [])


if __name__ == "__main__":
    unittest.main()
