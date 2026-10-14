"""Verify acceptance fixtures follow Cargo workspace membership."""

import json
from pathlib import Path
import subprocess
import tempfile
import unittest

from copy_workspace import copy_workspace


class CopyWorkspaceTests(unittest.TestCase):
    def test_new_parent_directories_globs_and_nested_members_load_in_the_copy(self):
        """New member locations and their local dependencies remain a valid workspace."""
        with tempfile.TemporaryDirectory(prefix="workspace fixture ") as temporary:
            source = Path(temporary) / "source with spaces"
            destination = Path(temporary) / "copied workspace"
            source.mkdir()
            (source / "Cargo.toml").write_text(
                '[workspace]\nmembers = ["future/*", "platform/mobile/client"]\n'
                'exclude = ["platform/mobile/vendor"]\nresolver = "3"\n',
                encoding="utf-8",
            )
            (source / "Cargo.lock").write_text("version = 4\n", encoding="utf-8")
            for name, directory in [
                ("future-member", "future/member"),
                ("mobile-client", "platform/mobile/client"),
                ("local-vendor", "platform/mobile/vendor"),
            ]:
                package = source / directory
                (package / "src").mkdir(parents=True)
                manifest = f'[package]\nname = "{name}"\nversion = "0.1.0"\nedition = "2024"\n'
                if name == "mobile-client":
                    manifest += '[dependencies]\nlocal-vendor = { path = "../vendor" }\n'
                (package / "Cargo.toml").write_text(manifest, encoding="utf-8")
                (package / "src/lib.rs").write_text("", encoding="utf-8")

            copy_workspace(source, destination)

            metadata = json.loads(subprocess.check_output(
                [
                    "cargo", "metadata", "--locked", "--offline", "--no-deps",
                    "--format-version", "1", "--manifest-path", str(destination / "Cargo.toml"),
                ],
                text=True,
            ))
            self.assertEqual(
                {package["name"] for package in metadata["packages"]},
                {"future-member", "mobile-client"},
            )
            self.assertTrue((destination / "platform/mobile/vendor/Cargo.toml").is_file())
            self.assertEqual((source / "Cargo.lock").read_bytes(), (destination / "Cargo.lock").read_bytes())


if __name__ == "__main__":
    unittest.main()
