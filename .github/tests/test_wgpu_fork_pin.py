"""Check that the wgpu fork pin parser and branch rule reject drift."""

import importlib.util
from pathlib import Path
import tomllib
import unittest


MODULE = Path(__file__).resolve().parents[1] / "scripts/wgpu_fork_pin.py"
SPEC = importlib.util.spec_from_file_location("wgpu_fork_pin", MODULE)
fork_pin = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(fork_pin)

URL = "https://github.com/cinnabar-mc/wgpu"


def manifest(*revs: str) -> dict:
    patches = {name: {"git": URL, "rev": rev} for name, rev in zip(("wgpu", "naga", "wgpu-hal"), revs)}
    patches["other"] = {"git": "https://github.com/example/other", "rev": "f" * 40}
    return {"workspace": {"dependencies": {"wgpu": {"version": "30.0.1"}}}, "patch": {"crates-io": patches}}


class WgpuForkPinTests(unittest.TestCase):
    def test_derives_the_release_branch_from_the_wgpu_major(self):
        self.assertEqual(fork_pin.pin(manifest("a" * 40, "a" * 40)), (URL.removeprefix(fork_pin.FORK_PREFIX), "a" * 40, "cinnabar-v30"))

    def test_rejects_crates_pinned_to_different_commits(self):
        with self.assertRaises(ValueError):
            fork_pin.pin(manifest("a" * 40, "b" * 40))

    def test_only_commits_contained_in_the_branch_pass(self):
        self.assertTrue(fork_pin.on_branch("identical"))
        self.assertTrue(fork_pin.on_branch("behind"))
        self.assertFalse(fork_pin.on_branch("ahead"))
        self.assertFalse(fork_pin.on_branch("diverged"))

    def test_the_workspace_manifest_pins_one_fork_commit(self):
        root = Path(__file__).resolve().parents[2]
        repository, rev, branch = fork_pin.pin(tomllib.loads((root / "Cargo.toml").read_text()))
        self.assertEqual(repository, URL.removeprefix(fork_pin.FORK_PREFIX))
        self.assertEqual(len(rev), 40)
        self.assertTrue(branch.startswith("cinnabar-v"))


if __name__ == "__main__":
    unittest.main()
