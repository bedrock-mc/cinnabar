"""Check median comparisons and reject incomplete Criterion artifacts."""

import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch


MODULE = Path(__file__).resolve().parents[1] / "scripts/runtime_profile.py"
SPEC = importlib.util.spec_from_file_location("runtime_profile", MODULE)
profile = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(profile)


class RuntimeSummaryTests(unittest.TestCase):
    """Use small Criterion-shaped artifacts without compiling or running benchmarks."""

    def setUp(self):
        """Give every variant one measured case in each requested bench target."""
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        for variant in profile.VARIANTS:
            artifact = self.root / f"runtime-profile-{variant}"
            artifact.mkdir()
            metadata = {
                **profile.configuration(variant, MODULE.parents[2]),
                "variant": variant,
                "head_sha": "same-revision",
                "sample_size": profile.SAMPLE_SIZE,
                "rustc": "same-compiler",
                "host": "same-host",
                "cpu": {"lscpu": [{"field": "Model name:", "data": "Test CPU"}]},
                "completed": True,
                "completed_benches": [
                    f"{package}/{bench}"
                    for package, benches in profile.BENCHES.items() for bench in benches
                ],
            }
            (artifact / "runtime-profile.json").write_text(json.dumps(metadata))
            for package, benches in profile.BENCHES.items():
                for bench in benches:
                    directory = artifact / "criterion" / package / bench / "case" / "new"
                    directory.mkdir(parents=True)
                    baseline = variant in ("baseline", "baseline-repeat")
                    self.write_case(directory, "case|one", 100 if baseline else 80)

    def write_case(self, directory, identity, median):
        """Write only the Criterion fields needed for a comparison."""
        (directory / "estimates.json").write_text(json.dumps({
            "median": {"point_estimate": median}, "mean": {"point_estimate": 999},
        }))
        (directory / "benchmark.json").write_text(json.dumps({"full_id": identity}))
        (directory / "sample.json").write_text(json.dumps({
            "times": [median] * profile.SAMPLE_SIZE, "iters": [1] * profile.SAMPLE_SIZE,
        }))

    def case_directory(self, variant):
        """Locate one case without repeating the configured target list."""
        return next((self.root / f"runtime-profile-{variant}").glob("criterion/**/new"))

    def test_medians_and_signed_change_use_baseline(self):
        """Report the median, with a faster candidate shown as a negative percentage."""
        report = profile.summary(self.root)
        self.assertIn("100.00 / +0.00%", report)
        self.assertIn("80.00 / -20.00%", report)
        self.assertNotIn("999.00", report)
        self.assertIn("case&#124;one", report)
        self.assertIn("Test CPU", report)

    def test_slower_variant_has_positive_change(self):
        """Show a slower candidate as a positive percentage rather than a speedup."""
        directory = self.case_directory("all")
        self.write_case(directory, "case|one", 120)
        self.assertIn("120.00 / +20.00%", profile.summary(self.root))

    def test_missing_variant_fails(self):
        """A missing artifact cannot produce a successful partial comparison."""
        (self.root / "runtime-profile-all" / "runtime-profile.json").unlink()
        with self.assertRaisesRegex(ValueError, "Incomplete results; all: missing"):
            profile.summary(self.root)

    def test_matching_partial_runs_fail(self):
        """Reject equal partial case sets and name every unfinished bench and variant."""
        for variant in profile.VARIANTS:
            path = self.root / f"runtime-profile-{variant}" / "runtime-profile.json"
            data = json.loads(path.read_text())
            del data["completed"]
            missing_bench = data["completed_benches"].pop()
            path.write_text(json.dumps(data))
        output = self.root / "summary.md"
        arguments = [str(MODULE), "summary", str(self.root), str(output)]
        with patch.object(sys, "argv", arguments):
            with self.assertRaises(SystemExit):
                profile.main()
        report = output.read_text()
        self.assertIn("Runtime measurements incomplete", report)
        self.assertNotIn("median (ns)", report)
        for variant in profile.VARIANTS:
            self.assertIn(f"{variant}: missing {missing_bench}", report)

    def test_completion_marker_is_required(self):
        """Reject a run whose commands are recorded but final completion is absent."""
        path = self.root / "runtime-profile-all" / "runtime-profile.json"
        data = json.loads(path.read_text())
        del data["completed"]
        path.write_text(json.dumps(data))
        with self.assertRaisesRegex(ValueError, "all: missing run completion marker"):
            profile.summary(self.root)

    def test_incomplete_report_records_each_failure(self):
        """Report every failed command, even when it saved some Criterion estimates."""
        path = self.root / "runtime-profile-all" / "runtime-profile.json"
        data = json.loads(path.read_text())
        del data["completed"]
        failed = data["completed_benches"][:2]
        data["completed_benches"] = data["completed_benches"][2:]
        data["failed_benches"] = {bench: "exit code 101" for bench in failed}
        path.write_text(json.dumps(data))
        output = self.root / "summary.md"
        with patch.object(sys, "argv", [str(MODULE), "summary", str(self.root), str(output)]):
            with self.assertRaises(SystemExit):
                profile.main()
        report = output.read_text()
        self.assertIn("Runtime measurements incomplete", report)
        self.assertNotIn("median (ns)", report)
        for bench in failed:
            self.assertIn(f"all: failed {bench}: exit code 101", report)

    def test_missing_bench_fails(self):
        """Every requested bench target must emit estimates."""
        (self.case_directory("fat-lto") / "estimates.json").unlink()
        with self.assertRaisesRegex(ValueError, "Missing estimates"):
            profile.summary(self.root)

    def test_different_cases_fail(self):
        """Variant coverage must match baseline rather than silently omitting a row."""
        path = self.case_directory("all") / "benchmark.json"
        path.write_text(json.dumps({"full_id": "different-case"}))
        with self.assertRaisesRegex(ValueError, "Different benchmark cases"):
            profile.summary(self.root)

    def test_group_sample_override_fails(self):
        """Detect a benchmark group that overrides the fixed experiment sample count."""
        path = self.case_directory("x86-64-v2") / "sample.json"
        path.write_text(json.dumps({"times": [1] * 10, "iters": [1] * 10}))
        with self.assertRaisesRegex(ValueError, "Wrong sample count"):
            profile.summary(self.root)

    def test_invalid_medians_fail(self):
        """Zero or nonfinite medians cannot yield meaningful percentage comparisons."""
        for median in [0, -1, float("nan"), float("inf")]:
            with self.subTest(median=median):
                path = self.case_directory("mimalloc") / "estimates.json"
                path.write_text(json.dumps({"median": {"point_estimate": median}}))
                with self.assertRaisesRegex(ValueError, "Invalid median"):
                    profile.summary(self.root)

    def test_different_revisions_fail(self):
        """Never compare an artifact from another source revision."""
        path = self.root / "runtime-profile-all" / "runtime-profile.json"
        data = json.loads(path.read_text())
        data["head_sha"] = "other-revision"
        path.write_text(json.dumps(data))
        with self.assertRaisesRegex(ValueError, "Different source revision"):
            profile.summary(self.root)

    def test_baseline_repeat_drift_rejects_comparison(self):
        """Reject either direction of no-op drift beyond the geomean tolerance."""
        directory = self.root / "runtime-profile-baseline-repeat"
        for change in (profile.BASELINE_REPEAT_LIMIT + 0.01, -profile.BASELINE_REPEAT_LIMIT - 0.01):
            for path in directory.glob("criterion/**/new"):
                self.write_case(path, "case|one", 100 * (1 + change))
            output = self.root / "summary.md"
            with patch.object(sys, "argv", [str(MODULE), "summary", str(self.root), str(output)]):
                with self.assertRaises(SystemExit):
                    profile.main()
            report = output.read_text()
            self.assertIn("Runtime comparison rejected", report)
            self.assertIn("Baseline repeat geomean change", report)
            self.assertNotIn("median (ns)", report)

    def test_baseline_repeat_at_limit_is_accepted(self):
        """Allow a no-op repeat exactly at either tolerance boundary."""
        directory = self.root / "runtime-profile-baseline-repeat"
        for change in (profile.BASELINE_REPEAT_LIMIT, -profile.BASELINE_REPEAT_LIMIT):
            for path in directory.glob("criterion/**/new"):
                self.write_case(path, "case|one", 100 * (1 + change))
            self.assertIn("median (ns)", profile.summary(self.root))

    def test_baseline_repeat_uses_geometric_mean(self):
        """Reciprocal changes cancel geometrically even when their arithmetic mean drifts."""
        paths = list((self.root / "runtime-profile-baseline-repeat").glob("criterion/**/new"))
        self.write_case(paths[0], "case|one", 50)
        self.write_case(paths[1], "case|one", 200)
        self.assertIn("median (ns)", profile.summary(self.root))

    def test_different_cpu_rejects_comparison(self):
        """Do not compare optimized binaries measured on a different CPU model."""
        path = self.root / "runtime-profile-all" / "runtime-profile.json"
        data = json.loads(path.read_text())
        data["cpu"]["lscpu"][0]["data"] = "Different CPU"
        path.write_text(json.dumps(data))
        with self.assertRaisesRegex(profile.ComparisonRejected, "Different measurement CPU"):
            profile.summary(self.root)

    def test_different_host_rejects_comparison(self):
        """A matching CPU model does not permit measurements from a different host."""
        path = self.root / "runtime-profile-all" / "runtime-profile.json"
        data = json.loads(path.read_text())
        data["host"] = "another-host"
        path.write_text(json.dumps(data))
        with self.assertRaisesRegex(profile.ComparisonRejected, "Different measurement host"):
            profile.summary(self.root)

    def test_unrequested_flags_reject_comparison(self):
        """Reject inherited compiler flags even if case coverage and hardware match."""
        path = self.root / "runtime-profile-fat-lto" / "runtime-profile.json"
        data = json.loads(path.read_text())
        data["rustflags"] = "-C opt-level=0"
        path.write_text(json.dumps(data))
        with self.assertRaisesRegex(profile.ComparisonRejected, "Unexpected rustflags"):
            profile.summary(self.root)


class RuntimeCompletionTests(unittest.TestCase):
    """Check the completion contract with mocked benchmark commands."""

    def setUp(self):
        """Prepare runner metadata inputs in a temporary workspace without building."""
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        (self.root / "Cargo.toml").write_text((MODULE.parents[2] / "Cargo.toml").read_text())
        self.results = self.root / "target"
        self.target = self.results / "runtime-profile-baseline"
        self.metadata = self.target / "runtime-profile.json"
        self.benches = [
            f"{package}/{bench}"
            for package, benches in profile.BENCHES.items() for bench in benches
        ]
        environment = {
            "CARGO_TARGET_DIR": str(self.results), "GITHUB_SHA": "same-revision",
            "GITHUB_STEP_SUMMARY": str(self.root / "audit.md"),
        }
        self.enterContext(patch.dict(os.environ, environment))
        self.enterContext(patch.object(
            profile.subprocess, "check_output", side_effect=["same-compiler", '{"lscpu": []}'],
        ))

    def test_success_marks_completion_after_all_commands(self):
        """Record completion only after the last successful benchmark command returns."""
        def succeed(*args, **kwargs):
            """Observe the pending marker while each simulated command is running."""
            self.assertNotIn("completed", json.loads(self.metadata.read_text()))

        with patch.object(profile.subprocess, "run", side_effect=succeed) as command:
            profile.run("baseline", self.root)
        metadata = json.loads(self.metadata.read_text())
        self.assertTrue(metadata["completed"])
        self.assertEqual(metadata["completed_benches"], self.benches)
        self.assertEqual(command.call_count, len(self.benches))
        self.assertEqual(metadata["failed_benches"], {})

    def test_metadata_failure_removes_previous_completion(self):
        """Discard a stale success marker even if runner metadata collection fails."""
        self.target.mkdir(parents=True)
        self.metadata.write_text(json.dumps({"completed": True}))
        with patch.object(profile.subprocess, "check_output", side_effect=OSError("missing tool")):
            with self.assertRaises(OSError):
                profile.run("baseline", self.root)
        self.assertFalse(self.metadata.exists())

    def test_later_failure_leaves_run_incomplete(self):
        """A final bench failure cannot retain a marker from an earlier successful run."""
        self.target.mkdir(parents=True)
        self.metadata.write_text(json.dumps({"completed": True, "completed_benches": self.benches}))
        outcomes = [None] * (len(self.benches) - 1) + [subprocess.CalledProcessError(1, "bench")]
        with patch.object(profile.subprocess, "run", side_effect=outcomes):
            with self.assertRaises(SystemExit):
                profile.run("baseline", self.root)
        metadata = json.loads(self.metadata.read_text())
        self.assertNotIn("completed", metadata)
        self.assertEqual(metadata["completed_benches"], self.benches[:-1])
        self.assertEqual(metadata["failed_benches"], {self.benches[-1]: "exit code 1"})

    def test_failed_benches_do_not_stop_later_targets(self):
        """Attempt later targets after both an unsuccessful exit and a launch failure."""
        failures = {
            self.benches[1]: "exit code 101",
            self.benches[3]: "could not start benchmark: missing executable",
        }
        outcomes = [None] * len(self.benches)
        outcomes[1] = subprocess.CalledProcessError(101, "bench")
        outcomes[3] = OSError("missing executable")
        with patch.object(profile.subprocess, "run", side_effect=outcomes) as command:
            with self.assertRaises(SystemExit):
                profile.run("baseline", self.root)
        metadata = json.loads(self.metadata.read_text())
        self.assertNotIn("completed", metadata)
        self.assertEqual(metadata["failed_benches"], failures)
        self.assertEqual(
            metadata["completed_benches"], [bench for bench in self.benches if bench not in failures],
        )
        attempted = []
        for call in command.call_args_list:
            arguments = call.args[0]
            package = arguments[arguments.index("-p") + 1]
            bench = arguments[arguments.index("--bench") + 1]
            attempted.append(f"{package}/{bench}")
        self.assertEqual(attempted, self.benches)

    def test_inherited_settings_are_removed_and_commands_are_audited(self):
        """Use only the controlled release settings and publish the executed command list."""
        inherited = {
            "CARGO_PROFILE_RELEASE_OPT_LEVEL": "0", "CARGO_PROFILE_RELEASE_DEBUG": "2",
            "CARGO_PROFILE_DEV_DEBUG": "2", "CARGO_ENCODED_RUSTFLAGS": "-Copt-level=0",
        }
        with patch.dict(os.environ, inherited):
            with patch.object(profile.subprocess, "run") as command:
                profile.run("baseline", self.root)
        config = profile.configuration("baseline", self.root)
        report = (self.root / "audit.md").read_text()
        self.assertIn("RUSTFLAGS: `(empty)`", report)
        self.assertIn("Profile: `release`", report)
        for call, entry in zip(command.call_args_list, config["commands"], strict=True):
            self.assertEqual(call.args[0], entry["command"])
            self.assertIn(profile.shlex.join(call.args[0]), report)
            env = call.kwargs["env"]
            self.assertNotIn("CARGO_ENCODED_RUSTFLAGS", env)
            self.assertNotIn("CARGO_PROFILE_DEV_DEBUG", env)
            self.assertEqual(env["RUSTFLAGS"], config["rustflags"])
            self.assertEqual(Path(env["CARGO_TARGET_DIR"]).resolve(), (self.target / "build").resolve())
            self.assertEqual(env["CARGO_BUILD_BUILD_DIR"], env["CARGO_TARGET_DIR"])
            for key, expected in config["profile_environment"].items():
                self.assertEqual(env[key], expected)

    def test_suite_continues_variants_and_releases_only_build_outputs(self):
        """Keep result JSON, isolate fresh builds, and attempt the baseline rebuild last."""
        def measure(variant, root):
            """Stand in for one variant while leaving its build and result files."""
            target = self.results / f"runtime-profile-{variant}"
            (target / "build").mkdir(parents=True)
            (target / "runtime-profile.json").write_text("{}")
            if variant == "fat-lto":
                raise SystemExit("a bench failed")

        with patch.object(profile, "run", side_effect=measure) as command:
            with self.assertRaises(SystemExit):
                profile.suite(self.root)
        self.assertEqual([call.args[0] for call in command.call_args_list], list(profile.VARIANTS))
        self.assertEqual(command.call_args_list[-1].args[0], "baseline-repeat")
        for variant in profile.VARIANTS:
            target = self.results / f"runtime-profile-{variant}"
            self.assertTrue((target / "runtime-profile.json").is_file())
            self.assertFalse((target / "build").exists())


class RuntimeConfigurationTests(unittest.TestCase):
    """Require only the requested settings to differ across benchmark configurations."""

    def test_repeat_rebuilds_the_same_baseline_configuration(self):
        """The repeat selects exactly the baseline commands, features, profile and flags."""
        self.assertEqual(
            profile.configuration("baseline", MODULE.parents[2]),
            profile.configuration("baseline-repeat", MODULE.parents[2]),
        )

    def test_each_candidate_changes_only_its_intended_knob(self):
        """Single-knob candidates cannot accidentally change unrelated compiler settings."""
        baseline = profile.configuration("baseline", MODULE.parents[2])
        effects = {
            "lto": {"lto", "profile_environment"}, "rustflags": {"rustflags"},
            "mimalloc": {"allocator", "commands"},
        }
        for variant, settings in profile.VARIANTS.items():
            allowed = set().union(*(effects[name] for name in settings))
            config = profile.configuration(variant, MODULE.parents[2])
            self.assertEqual({name for name in config if config[name] != baseline[name]}, allowed)
            expected_lto = str(config["lto"])
            env = {**baseline["profile_environment"], "CARGO_PROFILE_RELEASE_LTO": expected_lto}
            self.assertEqual(config["profile_environment"], env)


if __name__ == "__main__":
    unittest.main()
