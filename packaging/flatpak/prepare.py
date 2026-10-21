#!/usr/bin/env python3
"""Prepare a tracked source snapshot and vendored dependencies for an offline Flatpak build."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import tomllib
from urllib.request import urlopen
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[2]
TEMPLATE = Path(__file__).with_name("io.github.cinnabar_mc.cinnabar.json")


def run(*args, cwd, env=None):
    """Run a preparation command and return its text output, failing on errors."""
    return subprocess.check_output(args, cwd=cwd, env=env, text=True)


def snapshot(root, destination):
    """Copy tracked working files only, excluding ignored carriers, caches, and credentials."""
    paths = subprocess.check_output(["git", "ls-files", "-z"], cwd=root).split(b"\0")
    for raw in filter(None, paths):
        relative = Path(os.fsdecode(raw))
        source = root / relative
        target = destination / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, target, follow_symlinks=False)


def rust_module(target, manifest):
    """Use the workspace's exact Rust compiler and upstream component checksums."""
    components = ("rustc", "rust-std", "cargo")
    return {
        "name": "rust-toolchain",
        "buildsystem": "simple",
        "build-commands": [
            f"sh {name}/install.sh --prefix=/app/rust-toolchain --disable-ldconfig"
            for name in components
        ],
        "sources": [
            {
                "type": "archive",
                "url": manifest["pkg"][name]["target"][target]["xz_url"],
                "sha256": manifest["pkg"][name]["target"][target]["xz_hash"],
                "dest": name,
            }
            for name in components
        ],
    }


def prepare(output):
    """Generate a self-contained source archive and concrete x86-64 build manifest."""
    output.mkdir(parents=True, exist_ok=True)
    manifest = json.loads(TEMPLATE.read_text())
    with tempfile.TemporaryDirectory(prefix="cinnabar-flatpak-") as temporary:
        source = Path(temporary) / "cinnabar"
        source.mkdir()
        snapshot(ROOT, source)
        commit = run("git", "rev-parse", "HEAD", cwd=ROOT).strip()
        (source / "packaging/flatpak/source-commit").write_text(commit + "\n")
        print("Vendoring locked Rust dependencies…", flush=True)
        config = run("cargo", "vendor", "--quiet", "--locked", "--versioned-dirs", "vendor", cwd=source)
        (source / ".cargo/config.toml").write_text(config + "\n[net]\noffline = true\n")
        env = dict(os.environ, GOWORK="off", GOFLAGS="-mod=readonly")
        for module in ("core", "tools/localserver"):
            print(f"Vendoring {module} dependencies…", flush=True)
            run("go", "mod", "vendor", cwd=source / module, env=env)
        workspace = tomllib.loads((source / "Cargo.toml").read_text())
        metadata = source / f"packaging/flatpak/{manifest['app-id']}.metainfo.xml"
        tree = ET.parse(metadata)
        releases = ET.SubElement(tree.getroot(), "releases")
        ET.SubElement(releases, "release", version=workspace["workspace"]["package"]["version"])
        tree.write(metadata, encoding="utf-8", xml_declaration=True)
        channel = tomllib.loads((source / "rust-toolchain.toml").read_text())["toolchain"]["channel"]
        with urlopen(f"https://static.rust-lang.org/dist/channel-rust-{channel}.toml", timeout=60) as response:
            toolchain = tomllib.loads(response.read().decode())
        manifest["modules"].insert(0, rust_module("x86_64-unknown-linux-gnu", toolchain))
        archive = output / manifest["modules"][-1]["sources"][0]["path"]
        print("Writing the offline source archive…", flush=True)
        with tarfile.open(archive, "w:gz") as bundle:
            bundle.add(source, arcname="cinnabar")
    with archive.open("rb") as content:
        manifest["modules"][-1]["sources"][0]["sha256"] = hashlib.file_digest(content, "sha256").hexdigest()
    (output / TEMPLATE.name).write_text(json.dumps(manifest, indent=4) + "\n")
    print(output / TEMPLATE.name)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, default=ROOT / ".local/dist/flatpak")
    prepare(parser.parse_args().out.resolve())
