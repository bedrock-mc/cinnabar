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
        with self.assertRaisesRegex(RuntimeError, "Tracked sources changed.*value.txt"):
            cache.verify_inputs(self.root)

    def test_build_cannot_add_untracked_entries_to_tracked_directories(self):
        """Name the untracked entry that removed a directory from the input snapshot."""
        (self.root / "src/generated.rs").write_text("")
        with self.assertRaisesRegex(RuntimeError, "new entries: src/generated.rs"):
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

    def test_extra_cargo_outputs_are_cached_with_the_lane_profile(self):
        """A job building another profile or a cross target caches those outputs too."""
        env = {"RUNNER_OS": "Linux", "RUNNER_ARCH": "X64",
               "CACHE_CARGO_OUTPUTS": "release wasm32-unknown-unknown/release"}
        with patch.object(cache, "command", side_effect=["rustc 1", "revision"]):
            paths = cache.plan(self.root, "rust", "ci", env)["paths"].splitlines()
        for output in ["debug", "release", "wasm32-unknown-unknown/release"]:
            self.assertIn(str(self.root / "target" / output), paths)


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

    def setUp(self):
        """Use a small budget and isolated cache listing for retention checks."""
        self.env = {"CACHE_BUCKET": "linux-", "CACHE_KEY": "linux-current", "GITHUB_REF": "refs/heads/dev"}
        self.entries = [
            {"id": 1, "key": "linux-old", "ref": "refs/heads/dev", "created_at": "2026-01-01", "size_in_bytes": 20},
            {"id": 2, "key": "linux-current", "ref": "refs/heads/dev", "created_at": "2026-01-02", "size_in_bytes": 30},
        ]
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.budgets = Path(temporary.name) / "budgets.json"
        self.budgets.write_text(json.dumps({"cache": {"linux-": 30}}))
        for patcher in (patch.object(cache, "BUDGETS", self.budgets), patch.object(cache, "caches", return_value=self.entries)):
            patcher.start()
            self.addCleanup(patcher.stop)
        patcher = patch.object(cache, "delete_caches")
        self.deleted = patcher.start()
        self.addCleanup(patcher.stop)

    def test_save_preserves_the_predecessor(self):
        """Admission never removes a working cache before uploading its replacement."""
        self.entries.pop()
        cache.make_room(self.env)
        self.deleted.assert_not_called()

    def test_save_requires_room_for_both_entries(self):
        """A replacement cannot evict a predecessor by exceeding the shared storage limit."""
        self.entries[1]["ref"] = "refs/pull/1/merge"
        with patch.object(cache, "CACHE_LIMIT", 60):
            self.assertFalse(cache.make_room(self.env))
        self.deleted.assert_not_called()

    def test_admission_reserves_every_concurrent_bucket_replacement(self):
        """Separate upload decisions cannot each spend the same remaining repository space."""
        self.budgets.write_text(json.dumps({"cache": {"linux-": 30, "other-": 20}}))
        self.entries[1]["ref"] = "refs/pull/1/merge"
        with patch.object(cache, "CACHE_LIMIT", 90):
            self.assertFalse(cache.make_room(self.env))
        self.deleted.assert_not_called()

    def test_interrupted_pruning_does_not_block_all_future_uploads(self):
        """An already validated replacement lets admission retire a stranded predecessor."""
        with patch.object(cache, "CACHE_LIMIT", 60):
            self.assertTrue(cache.make_room(self.env))
        self.deleted.assert_called_once_with(self.env, [1])

    def test_reconciliation_keeps_the_newest_valid_entry_on_each_ref(self):
        """A completed newer save protects a working cache through cleanup of stranded entries."""
        self.entries.extend([
            {**self.entries[1], "id": 3, "key": "linux-newer", "created_at": "2026-01-03"},
            {**self.entries[0], "id": 4, "key": "other-old"},
            {**self.entries[1], "id": 5, "key": "linux-another-ref", "ref": "refs/pull/1/merge"},
        ])
        cache.make_room(self.env)
        self.deleted.assert_called_once_with(self.env, [1, 2])

    def test_reconciliation_can_discard_a_stranded_oversized_upload(self):
        """An oversized interrupted upload cannot block replacement of the retained valid cache."""
        self.entries[1]["size_in_bytes"] = 31
        with patch.object(cache, "CACHE_LIMIT", 60):
            self.assertTrue(cache.make_room(self.env))
        self.deleted.assert_called_once_with(self.env, [2])

    def test_reconciliation_preserves_the_only_oversized_predecessor(self):
        """Admission cannot remove a sole working predecessor without a validated replacement."""
        self.entries.pop()
        self.entries[0]["size_in_bytes"] = 31
        with patch.object(cache, "CACHE_LIMIT", 60):
            self.assertFalse(cache.make_room(self.env))
        self.deleted.assert_not_called()

    def test_admission_outputs_control_the_upload_step(self):
        """The composite action receives an explicit decision even when storage is full."""
        self.entries[1]["ref"] = "refs/pull/1/merge"
        for repository_limit, expected in ((80, "true"), (60, "false")):
            with self.subTest(repository_limit=repository_limit), tempfile.TemporaryDirectory() as temporary:
                output = Path(temporary) / "output"
                with patch.dict(os.environ, {**self.env, "GITHUB_OUTPUT": str(output)}), \
                        patch.object(cache.sys, "argv", [str(MODULE), "make-room"]), \
                        patch.object(cache, "CACHE_LIMIT", repository_limit):
                    cache.main()
                self.assertEqual(output.read_text(), f"save={expected}\n")
        self.deleted.assert_not_called()

    def test_oversized_replacement_removes_only_itself(self):
        """Rejecting a saved archive must leave the last valid entry available."""
        self.entries[1]["size_in_bytes"] = 31
        with self.assertRaises(cache.BudgetError):
            cache.prune(self.env)
        self.deleted.assert_called_once_with(self.env, [2])

    def test_missing_replacement_preserves_the_predecessor(self):
        """A skipped or failed upload never authorizes retention cleanup."""
        self.entries.pop()
        cache.prune(self.env)
        self.deleted.assert_not_called()

    def test_wrong_ref_does_not_validate_a_replacement(self):
        """An entry with the same key on another ref cannot replace this ref's cache."""
        self.entries[1]["ref"] = "refs/pull/1/merge"
        cache.prune(self.env)
        self.deleted.assert_not_called()

    def test_valid_replacement_retires_only_older_entries(self):
        """Validated saves preserve concurrent newer writes and unrelated buckets."""
        self.entries.extend([
            {**self.entries[0], "id": 3, "key": "linux-newer", "created_at": "2026-01-03"},
            {**self.entries[0], "id": 4, "key": "macos-old"},
        ])
        cache.prune(self.env)
        self.deleted.assert_called_once_with(self.env, [1])


class BudgetTests(unittest.TestCase):
    """Every saved bucket has a budget, and the GitHub cache budgets fit its limit together."""

    def test_cache_budgets_fit_the_repository_limit(self):
        budgets = json.loads(cache.BUDGETS.read_text())
        self.assertLessEqual(2 * sum(budgets["cache"].values()), cache.CACHE_LIMIT)
        self.assertFalse(budgets["cache"].keys() & budgets["artifact"].keys())
        for store in ("cache", "artifact"):
            for bucket, size in budgets[store].items():
                self.assertTrue(bucket.startswith("build-v1-") and bucket.endswith("-"), bucket)
                self.assertGreater(size, 0, bucket)

    def test_observed_ci_archives_fit_their_budgets(self):
        """The largest observed Rust and Go archives must survive retention."""
        self.assertLessEqual(2_791_327_868, cache.budget("artifact", "build-v1-rust-Linux-X64-ci-"))
        self.assertLessEqual(2_555_389_355, cache.budget("artifact", "build-v1-rust-Windows-X64-ci-"))
        self.assertLessEqual(857_488_115, cache.budget("artifact", "build-v1-go-Linux-X64-ci-"))

    def test_release_caches_stay_out_of_the_shared_cache(self):
        """Rarely used release entries would be the first evicted from the shared cache."""
        budgets = json.loads(cache.BUDGETS.read_text())
        self.assertTrue(all(bucket.endswith("-ci-") for bucket in budgets["cache"]))
        self.assertTrue(all(bucket.endswith(("-release-", "-ci-")) for bucket in budgets["artifact"]))

    def test_unbudgeted_buckets_cannot_be_saved(self):
        with self.assertRaisesRegex(cache.BudgetError, "has no cache budget"):
            cache.budget("cache", "build-v1-rust-Plan9-X64-ci-")


class StorageTests(unittest.TestCase):
    """Store selection follows the single budget manifest, including saves from existing callers."""

    def test_every_bucket_selects_its_budgeted_store(self):
        """Changing a budget's store changes both restore and save decisions together."""
        for store, budgets in json.loads(cache.BUDGETS.read_text()).items():
            for bucket in budgets:
                with self.subTest(bucket=bucket):
                    prefix = bucket + "compiler-"
                    selected = cache.storage({"CACHE_BUCKET": bucket, "CACHE_KEY": prefix + "revision-123-1"})
                    self.assertEqual(selected, {"store": store, "prefix": prefix})

    def test_unbudgeted_buckets_fail_before_uploading(self):
        """An unknown lane cannot silently use unallocated shared space."""
        with self.assertRaises(cache.BudgetError):
            cache.storage({"CACHE_BUCKET": "unknown-", "CACHE_KEY": "unknown-compiler-revision-123-1"})

    def test_explicit_release_store_and_prefix_are_preserved(self):
        """Release callers retain their explicitly selected store and compatible prefix."""
        bucket = next(iter(json.loads(cache.BUDGETS.read_text())["artifact"]))
        self.assertEqual(cache.storage({"CACHE_STORE": "artifact", "CACHE_BUCKET": bucket, "CACHE_PREFIX": "given-"}),
                         {"store": "artifact", "prefix": "given-"})


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

    def test_symlinks_such_as_uplifted_dsym_bundles_are_left_out_so_the_restore_succeeds(self):
        with tempfile.TemporaryDirectory() as temp:
            source, archive = Path(temp) / "runner", Path(temp) / "cache.tar.gz"
            target = source / "target/release"
            bundle = target / "deps/cinnabar-0123.dSYM/Contents/Resources/DWARF"
            bundle.mkdir(parents=True)
            (bundle / "cinnabar-0123").write_bytes(b"dwarf")
            try:
                (target / "cinnabar.dSYM").symlink_to(target / "deps/cinnabar-0123.dSYM",
                                                      target_is_directory=True)
            except OSError as error:
                self.skipTest(f"missing fixture: symbolic link support ({error})")
            paths = [str(target)]
            cache.pack(paths, archive)
            shutil.rmtree(source)

            with open(archive, "rb") as stream:
                cache.unpack(paths, stream)

            self.assertEqual((bundle / "cinnabar-0123").read_bytes(), b"dwarf")
            self.assertFalse(os.path.lexists(target / "cinnabar.dSYM"))

    def test_linked_executables_are_left_out_on_every_platform(self):
        """Test and bin executables would push the Rust archive over its budget; libraries stay."""
        kept = [
            "deps/libdemo-0123.rlib", "deps/libdemo-0123.rmeta", "deps/demo-0123.d",
            "deps/libmacro-0123.so", "deps/libmacro-0123.dylib", "deps/macro-0123.dll",
            "deps/macro-0123.dll.lib", "libdemo.rlib", "demo.d",
            "build/sys-0123/build-script-build", "build/sys-0123/out/libnative.a",
            ".fingerprint/demo-0123/test-lib-demo",
        ]
        left_out = [
            "deps/demo-4567", "deps/demo-4567.exe", "deps/demo-4567.pdb",
            "deps/demo-4567.demo.abc-cgu.0.rcgu.o", "deps/demo-4567.dSYM/Contents/Info.plist",
            "demo", "demo.exe", "demo.pdb", "demo.dSYM/Contents/Info.plist",
            "examples/sample-0123", "incremental/demo-0123/s-1/query-cache.bin",
        ]
        go_cache = ["00/0011-a", "00/0011-d"]
        for store in ("artifact", "cache"):
            with self.subTest(store=store), tempfile.TemporaryDirectory() as temp:
                source, archive = Path(temp) / "runner", Path(temp) / "cache.tar.gz"
                profile, go = source / "target/debug", source / "go-build"
                for name in kept + left_out:
                    (profile / name).parent.mkdir(parents=True, exist_ok=True)
                    (profile / name).write_bytes(b"x" * 10)
                for name in go_cache:
                    (go / name).parent.mkdir(parents=True, exist_ok=True)
                    (go / name).write_bytes(b"go")
                paths = [str(profile), str(go)]
                if store == "artifact":
                    sizes = cache.pack(paths, archive)
                    shutil.rmtree(source)
                    with open(archive, "rb") as stream:
                        cache.unpack(paths, stream)
                else:
                    sizes = cache.drop_linked_outputs(paths)

                restored = {path.relative_to(profile).as_posix()
                            for path in profile.rglob("*") if path.is_file()}
                self.assertEqual(restored, set(kept))
                self.assertTrue(all((go / name).exists() for name in go_cache))
                self.assertEqual(sum(sizes.left_out.values()), 10 * len(left_out))


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

    REPOSITORY = json.loads(
        (Path(__file__).resolve().parents[2] / "packaging/release-assets.json").read_text()
    )["repository"]
    ENV = {"GITHUB_WORKFLOW_REF": f"{REPOSITORY}/.github/workflows/package.yml@refs/heads/dev",
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

    def test_ci_and_its_warmer_share_only_trusted_default_branch_artifacts(self):
        """CI can restore the warmer's archive without trusting unrelated workflows or forks."""
        env = {**self.ENV, "CACHE_BUCKET": "build-v1-rust-Linux-X64-ci-",
               "GITHUB_WORKFLOW_REF": self.ENV["GITHUB_WORKFLOW_REF"].replace("package.yml", "ci.yml")}
        run = self.run_(path=".github/workflows/ci-cache.yml")
        self.assertTrue(cache.trusted_run(run, env))
        self.assertFalse(cache.trusted_run({**run, "head_repository": {"id": 99}}, env))
        self.assertFalse(cache.trusted_run({**run, "path": ".github/workflows/package.yml"}, env))
        self.assertFalse(cache.trusted_run({**run, "event": "pull_request"}, env))
        self.assertFalse(cache.trusted_run({**run, "head_branch": "feature"}, env))
        for workflow in cache.CI_WORKFLOWS:
            self.assertTrue((MODULE.parents[3] / workflow).is_file(), workflow)

    def test_artifact_listing_accepts_attempt_names_and_legacy_names(self):
        """Unique uploads remain discoverable by compiler prefix without matching unrelated artifacts."""
        entries = [{"id": index, "name": name, "expired": False, "created_at": f"2026-01-0{index}",
                    "workflow_run": {"id": index}} for index, name in enumerate(
                        ["compiler", "compiler-123-1", "different-compiler-123-1"], start=1)]
        def api(*arguments):
            """Return artifact pages or the trusted workflow metadata for the requested resource."""
            return json.dumps([{"artifacts": entries}] if "artifacts?" in arguments[-1] else self.run_())
        with patch.object(cache, "command", side_effect=api):
            self.assertEqual([entry["id"] for entry in cache.artifacts({**self.ENV, "GITHUB_REPOSITORY": self.REPOSITORY}, "compiler")],
                             [2, 1])

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

    def test_missing_uploaded_artifact_preserves_both_predecessor_stores(self):
        """A failed upload or delayed listing cannot authorize any cache deletion."""
        with tempfile.TemporaryDirectory() as temporary:
            env = {"CACHE_ARCHIVE": str(Path(temporary) / "archive"), "CACHE_PREFIX": "linux-compiler-",
                   "CACHE_ARTIFACT_ID": "20"}
            with patch.object(cache, "artifacts", return_value=[]), patch.object(cache, "delete") as deleted, \
                    patch.object(cache, "delete_caches") as deleted_caches:
                cache.prune_artifacts(env)
            deleted.assert_not_called()
            deleted_caches.assert_not_called()

    def test_concurrent_writer_can_retire_this_upload_before_pruning(self):
        """A newer successful writer may remove this upload before its own prune step runs."""
        with tempfile.TemporaryDirectory() as temporary:
            env = {"CACHE_ARCHIVE": str(Path(temporary) / "archive"), "CACHE_PREFIX": "linux-ci-compiler-",
                   "CACHE_BUCKET": "linux-ci-", "CACHE_ARTIFACT_ID": "20", "GITHUB_REF": "refs/heads/dev",
                   "GITHUB_REPOSITORY": "owner/repo"}
            newer = [{"id": 30, "created_at": "2026-01-03"}]
            with patch.object(cache, "artifacts", return_value=newer), patch.object(cache, "delete") as deleted, \
                    patch.object(cache, "delete_caches") as deleted_caches:
                cache.prune_artifacts(env)
            deleted.assert_not_called()
            deleted_caches.assert_not_called()

    def test_valid_artifact_retires_only_older_entries_from_either_store(self):
        """A migration retains other refs, other buckets and concurrent newer saves."""
        with tempfile.TemporaryDirectory() as temporary:
            env = {"CACHE_ARCHIVE": str(Path(temporary) / "archive"), "CACHE_PREFIX": "linux-ci-compiler-",
                   "CACHE_BUCKET": "linux-ci-", "CACHE_ARTIFACT_ID": "20", "GITHUB_REF": "refs/heads/dev",
                   "GITHUB_REPOSITORY": "owner/repo"}
            found = [{"id": 10, "created_at": "2026-01-01"}, {"id": 20, "created_at": "2026-01-02"},
                     {"id": 30, "created_at": "2026-01-03"}]
            old = {"id": 1, "key": "linux-ci-old", "ref": env["GITHUB_REF"], "created_at": "2026-01-01"}
            entries = [old, {**old, "id": 2, "ref": "refs/pull/1/merge"}, {**old, "id": 3, "key": "other-ci-old"},
                       {**old, "id": 4, "created_at": "2026-01-03"}]
            with patch.object(cache, "artifacts", return_value=found), patch.object(cache, "caches", return_value=entries), \
                    patch.object(cache, "delete") as deleted, patch.object(cache, "delete_caches") as deleted_caches:
                cache.prune_artifacts(env)
            deleted.assert_called_once_with("repos/owner/repo/actions/artifacts/10")
            deleted_caches.assert_called_once_with(env, [1])

    def test_reruns_prepare_distinct_artifact_names(self):
        """An upload for a later attempt cannot overwrite the earlier attempt's valid archive."""
        with tempfile.TemporaryDirectory() as temporary:
            env = {"RUNNER_TEMP": temporary, "CACHE_PREFIX": "compiler-", "CACHE_BUCKET": "linux-ci-",
                   "CACHE_PATHS": "source", "GITHUB_RUN_ID": "123"}
            names = []
            def pack(paths, archive):
                """Create a small compressed archive without invoking a compiler."""
                archive.write_bytes(b"archive")
            for attempt in ("1", "2"):
                output = Path(temporary) / f"output-{attempt}"
                with patch.object(cache, "pack", side_effect=pack), patch.object(cache, "budget", return_value=30):
                    cache.pack_artifact({**env, "GITHUB_RUN_ATTEMPT": attempt, "GITHUB_OUTPUT": str(output)})
                names.append(dict(line.split("=", 1) for line in output.read_text().splitlines())["name"])
            self.assertNotEqual(*names)
            self.assertTrue(all((Path(temporary) / f"{name}.tar.gz").is_file() for name in names))

    def test_oversized_archive_is_rejected_before_publication(self):
        """A compressed candidate over budget cannot replace an existing valid archive."""
        with tempfile.TemporaryDirectory() as temporary:
            env = {"RUNNER_TEMP": temporary, "CACHE_PREFIX": "linux-ci-compiler-", "CACHE_BUCKET": "linux-ci-",
                   "CACHE_PATHS": "source"}
            def pack(paths, archive):
                """Write an already compressed fixture archive that exceeds the allocation."""
                archive.write_bytes(b"x" * 31)
            with patch.object(cache, "pack", side_effect=pack), patch.object(cache, "budget", return_value=30), \
                    self.assertRaises(cache.BudgetError):
                cache.pack_artifact(env)
            self.assertFalse(list(Path(temporary).iterdir()))


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
