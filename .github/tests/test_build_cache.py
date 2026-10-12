"""Check artifact reuse, changed-input invalidation, and bounded cache retention."""

import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
import tarfile
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

    def test_native_cache_retention_preserves_cross_targets(self):
        """Regular native CI must not prune release caches built for explicit targets."""
        root = Path("source").resolve()
        env = {"RUNNER_OS": "macOS", "RUNNER_ARCH": "ARM64"}
        for language, settings, outputs in [
            ("rust", {"CARGO_BUILD_TARGET": "x86_64-apple-darwin"}, ["rustc 1", "revision"]),
            ("go", {"GOOS": "darwin", "GOARCH": "amd64"}, ["go version 1", "cache\nmodules", "revision"]),
        ]:
            with patch.object(cache, "command", side_effect=outputs * 2):
                native = cache.plan(root, language, "ci", env)
                cross = cache.plan(root, language, "ci", {**env, **settings})
            entries = [
                {"id": 1, "key": cross["key"], "ref": "refs/heads/dev", "created_at": "2026-01-01"},
                {"id": 2, "key": native["key"], "ref": "refs/heads/dev", "created_at": "2026-01-02"},
            ]
            self.assertEqual(cache.superseded(entries, native["key"], native["bucket"], "refs/heads/dev"), [])


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

    def test_saving_replaces_every_entry_in_its_bucket_first(self):
        """Old and new entries never coexist, so a save cannot push the total over the limit."""
        entries = [
            {"id": 1, "key": "linux-old", "ref": "refs/heads/dev"},
            {"id": 2, "key": "linux-older", "ref": "refs/heads/dev"},
            {"id": 3, "key": "macos-old", "ref": "refs/heads/dev"},
            {"id": 4, "key": "linux-old", "ref": "refs/pull/1/merge"},
        ]
        self.assertEqual(cache.replaced(entries, "linux-", "refs/heads/dev"), [1, 2])


class BudgetTests(unittest.TestCase):
    """Every saved bucket has a budget, and the GitHub cache budgets fit its limit together."""

    def test_cache_budgets_fit_the_repository_limit(self):
        budgets = json.loads(cache.BUDGETS.read_text())
        self.assertLessEqual(sum(budgets["cache"].values()), cache.CACHE_LIMIT)
        for store in ("cache", "artifact"):
            for bucket, size in budgets[store].items():
                self.assertTrue(bucket.startswith("build-v1-") and bucket.endswith("-"), bucket)
                self.assertGreater(size, 0, bucket)

    def test_release_caches_stay_out_of_the_shared_cache(self):
        """Rarely used release entries would be the first evicted from the shared cache."""
        budgets = json.loads(cache.BUDGETS.read_text())
        self.assertTrue(all(bucket.endswith("-ci-") for bucket in budgets["cache"]))
        self.assertTrue(all(bucket.endswith("-release-") for bucket in budgets["artifact"]))

    def test_unbudgeted_buckets_cannot_be_saved(self):
        with self.assertRaisesRegex(cache.BudgetError, "has no cache budget"):
            cache.budget("cache", "build-v1-rust-Plan9-X64-ci-")


class ArchiveTests(unittest.TestCase):
    """Artifact caches must restore paths, timestamps and modes as the cache action does."""

    def test_round_trip_restores_exact_timestamps_links_and_read_only_directories(self):
        with tempfile.TemporaryDirectory() as temp:
            source, archive = Path(temp) / "runner", Path(temp) / "cache.tar.gz"
            target = source / "target/release"
            (target / "deps").mkdir(parents=True)
            (target / "deps/libdemo.rlib").write_bytes(b"rlib")
            os.link(target / "deps/libdemo.rlib", target / "libdemo.rlib")
            os.utime(target / "deps/libdemo.rlib", ns=(1_700_000_000_123_456_789,) * 2)
            (source / "target/.ci-inputs.json").write_text("{}")
            module = source / "home/registry/src/index/zstd-sys-2.0.0"
            module.mkdir(parents=True)
            (module / "build.rs").write_text("fn main() {}")
            (source / "home/registry/src/index/serde-1.0.0").mkdir()
            module.chmod(0o555)
            paths = [str(target), str(source / "target/.ci-inputs.json"),
                     str(source / "home/registry/src/*/*-sys-*")]
            cache.pack(paths, archive)
            module.chmod(0o755)
            shutil.rmtree(source)

            with open(archive, "rb") as stream:
                cache.unpack(paths, stream)

            restored = target / "deps/libdemo.rlib"
            self.assertEqual(restored.read_bytes(), b"rlib")
            self.assertAlmostEqual(restored.stat().st_mtime, 1_700_000_000.123456789, places=5)
            self.assertEqual(restored.stat().st_ino, (target / "libdemo.rlib").stat().st_ino)
            self.assertEqual((source / "target/.ci-inputs.json").read_text(), "{}")
            self.assertEqual((module / "build.rs").read_text(), "fn main() {}")
            self.assertEqual(stat.S_IMODE(module.stat().st_mode), 0o555)
            self.assertFalse((source / "home/registry/src/index/serde-1.0.0").exists())
            module.chmod(0o755)


    def test_entries_outside_their_root_are_rejected(self):
        """A tampered archive cannot write outside the planned paths or plant links and devices."""
        def entry(name, kind=tarfile.REGTYPE, linkname=""):
            info = tarfile.TarInfo(name)
            info.type, info.linkname = kind, linkname
            return info

        for bad in [
            entry("0/../escape.txt"),
            entry("0//absolute.txt"),
            entry("0//tmp/build-cache-outside", tarfile.DIRTYPE),
            entry("0/nested/../../escape", tarfile.DIRTYPE),
            entry("7/unplanned.txt"),
            entry("0/link", tarfile.SYMTYPE, "/etc/passwd"),
            entry("0/device", tarfile.CHRTYPE),
            entry("0/hard", tarfile.LNKTYPE, "1/elsewhere"),
            entry("0/hard", tarfile.LNKTYPE, "0/../../outside"),
        ]:
            with tempfile.TemporaryDirectory() as temp:
                root, archive = Path(temp) / "cache/target", Path(temp) / "cache.tar.gz"
                with tarfile.open(archive, "w:gz") as tar:
                    tar.addfile(bad, io.BytesIO(b"") if bad.isreg() else None)
                with open(archive, "rb") as stream, self.assertRaises(tarfile.FilterError, msg=bad.name):
                    cache.unpack([str(root), str(Path(temp) / "cache/other")], stream)
                self.assertFalse((Path(temp) / "escape.txt").exists())
                self.assertFalse((Path(temp) / "cache/escape").exists())
                self.assertFalse(root.exists(), "nothing is extracted before the entry is refused")
                self.assertFalse(Path("/absolute.txt").exists())
                self.assertFalse(Path("/tmp/build-cache-outside").exists())


class ProvenanceTests(unittest.TestCase):
    """Release jobs restore only artifacts this repository's own workflow saved on dev."""

    ENV = {"GITHUB_WORKFLOW_REF": "bedrock-mc/cinnabar/.github/workflows/package.yml@refs/heads/dev",
           "GITHUB_REPOSITORY_ID": "7", "CACHE_BRANCH": "dev"}

    def run_(self, **changes):
        run = {"event": "schedule", "head_branch": "dev", "path": ".github/workflows/package.yml",
               "repository": {"id": 7}, "head_repository": {"id": 7}}
        return {**run, **changes}

    def select(self, runs):
        listed = [{"id": run_id, "expired": False, "created_at": f"2026-01-0{run_id}",
                   "workflow_run": {"id": run_id}} for run_id in runs]
        return [artifact["id"] for artifact in cache.trusted_artifacts(listed, runs.get, self.ENV)]

    def test_only_this_workflow_on_dev_from_this_repository_is_trusted(self):
        runs = {
            1: self.run_(),
            2: self.run_(event="workflow_dispatch"),
            3: self.run_(head_repository={"id": 99}),
            4: self.run_(event="pull_request"),
            5: self.run_(event="pull_request_target"),
            6: self.run_(head_branch="feature"),
            7: self.run_(path=".github/workflows/ci.yml"),
        }
        self.assertEqual(self.select(runs), [2, 1])

    def test_listing_failures_leave_a_cold_build(self):
        with tempfile.TemporaryDirectory() as temp:
            env = {"CACHE_PATHS": str(Path(temp) / "target"), "CACHE_PREFIX": "bucket-",
                   "RUNNER_TEMP": temp}
            failure = subprocess.CalledProcessError(1, ["gh", "api"], stderr="HTTP 502")
            with patch.object(cache, "artifacts", side_effect=failure):
                cache.restore_artifact(env)


class ArtifactRetentionTests(unittest.TestCase):
    """Concurrent nightly and release runs may both upload the same cache name."""

    def test_only_artifacts_older_than_the_replacement_are_deleted(self):
        found = [
            {"id": 30, "created_at": "2026-01-03"},
            {"id": 20, "created_at": "2026-01-02"},
            {"id": 10, "created_at": "2026-01-01"},
        ]
        self.assertEqual(cache.superseded_artifacts(found, 20), [10])
        self.assertEqual(cache.superseded_artifacts(found, 99), [])


class DeletionTests(unittest.TestCase):
    """Two workflows replacing the same bucket may race to delete one entry."""

    def test_already_deleted_entries_are_not_errors(self):
        missing = subprocess.CompletedProcess(["gh"], 1, "", "gh: Not Found (HTTP 404)")
        with patch.object(cache.subprocess, "run", return_value=missing):
            cache.delete("repos/o/r/actions/caches/1")
        failing = subprocess.CompletedProcess(["gh"], 1, "", "gh: Server Error (HTTP 500)")
        with patch.object(cache.subprocess, "run", return_value=failing), \
                self.assertRaises(subprocess.CalledProcessError):
            cache.delete("repos/o/r/actions/caches/1")


if __name__ == "__main__":
    unittest.main()
