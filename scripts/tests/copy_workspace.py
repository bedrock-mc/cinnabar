"""Copy a virtual workspace's member trees for acceptance provenance tests."""

import json
from pathlib import Path
import shutil
import subprocess
import sys


def copy_workspace(source: Path, destination: Path) -> None:
    """Copy the root manifests and parent trees of Cargo's resolved workspace members."""
    source = source.resolve()
    metadata = json.loads(subprocess.check_output(
        [
            "cargo", "metadata", "--locked", "--offline", "--no-deps",
            "--format-version", "1", "--manifest-path", str(source / "Cargo.toml"),
        ],
        cwd=source,
        encoding="utf-8",
    ))
    members = set(metadata["workspace_members"])
    directories = {
        Path(package["manifest_path"]).relative_to(source).parts[0]
        for package in metadata["packages"]
        if package["id"] in members
    }
    destination.mkdir(parents=True, exist_ok=True)
    for name in ("Cargo.toml", "Cargo.lock"):
        shutil.copy2(source / name, destination / name)
    for directory in sorted(directories):
        shutil.copytree(source / directory, destination / directory)


if __name__ == "__main__":
    copy_workspace(Path(sys.argv[1]), Path(sys.argv[2]))
