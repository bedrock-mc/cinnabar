#!/usr/bin/env python3
"""Read or bump the workspace release version without changing dependencies."""

import argparse
import os
from pathlib import Path
import re
import sys
import tempfile
import tomllib


SEMVER = re.compile(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)")


def inherited_packages(root: Path, workspace: dict) -> set[str]:
    excluded = {
        path.resolve()
        for pattern in workspace.get("exclude", [])
        for path in root.glob(pattern)
    }
    packages = set()
    for pattern in workspace["members"]:
        members = [path for path in root.glob(pattern) if path.resolve() not in excluded]
        if not members:
            raise ValueError(f"workspace member pattern has no matches: {pattern}")
        for member in members:
            manifest = tomllib.loads((member / "Cargo.toml").read_text())
            package = manifest["package"]
            if package.get("version") == {"workspace": True}:
                packages.add(package["name"])
    package = workspace.get("root_package", {})
    if package.get("version") == {"workspace": True}:
        packages.add(package["name"])
    return packages


def version_line(text: str, current: str, replacement: str) -> str:
    pattern = re.compile(r'(?m)^(\s*version\s*=\s*["\'])' + re.escape(current) + r'(["\'])')
    updated, count = pattern.subn(lambda match: match[1] + replacement + match[2], text)
    if count != 1:
        raise ValueError("expected one version assignment in the package table")
    return updated


def lockfile_versions(text: str, names: set[str], current: str, replacement: str) -> str:
    seen = set()
    blocks = re.split(r"(?m)(?=^\[\[package\]\]\s*$)", text)
    for index, block in enumerate(blocks):
        if not block.startswith("[[package]]"):
            continue
        package = tomllib.loads(block)["package"][0]
        name = package["name"]
        if name in names and "source" not in package:
            if name in seen or package["version"] != current:
                raise ValueError(f"Cargo.lock version mismatch for workspace package {name}")
            seen.add(name)
            block = version_line(block, current, replacement)
        # Ambiguous package names carry a version in their dependency references.
        # References with an explicit registry/git source belong to dependencies.
        for dependency in package.get("dependencies", []):
            parts = dependency.split()
            if len(parts) == 2 and parts[0] in names and parts[1] == current:
                old = re.escape(dependency)
                block = re.sub(
                    r'(?m)^(\s*")' + old + r'("\s*,?\s*)$',
                    lambda match: match[1] + parts[0] + " " + replacement + match[2],
                    block,
                )
        blocks[index] = block
    missing = names - seen
    if missing:
        raise ValueError("Cargo.lock is missing workspace packages: " + ", ".join(sorted(missing)))
    return "".join(blocks)


def atomic_write(path: Path, text: str) -> None:
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode="w", dir=path.parent, delete=False) as output:
            temporary = Path(output.name)
            output.write(text)
            output.flush()
            os.fsync(output.fileno())
            os.chmod(temporary, path.stat().st_mode & 0o777)
        os.replace(temporary, path)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def bump(root: Path, kind: str, dry_run: bool, custom: str = "") -> str:
    manifest_path = root / "Cargo.toml"
    lock_path = root / "Cargo.lock"
    text = manifest_path.read_text()
    manifest = tomllib.loads(text)
    workspace = manifest["workspace"]
    current = workspace["package"]["version"]
    match = SEMVER.fullmatch(current)
    if not match:
        raise ValueError("workspace.package.version must be a plain semantic version, such as 1.2.3")
    version = list(map(int, match.groups()))
    if kind == "custom":
        if not SEMVER.fullmatch(custom):
            raise ValueError("custom version must be a plain semantic version, such as 1.2.3")
        version = list(map(int, custom.split(".")))
        if tuple(version) <= tuple(map(int, match.groups())):
            raise ValueError("custom version must be greater than the current version")
    elif kind != "current":
        component = {"major": 0, "minor": 1, "patch": 2}[kind]
        version[component] += 1
        version[component + 1 :] = [0] * (2 - component)
    replacement = ".".join(map(str, version))
    workspace = dict(workspace, root_package=manifest.get("package", {}))
    names = inherited_packages(root, workspace)
    lock = lockfile_versions(lock_path.read_text(), names, current, replacement)
    tables = re.split(r"(?m)(?=^\[)", text)
    found = False
    for index, table in enumerate(tables):
        if re.match(r"\[workspace\.package\]\s*(?:#.*)?\n", table):
            tables[index] = version_line(table, current, replacement)
            found = True
    if not found:
        raise ValueError("could not locate the workspace.package version table")
    if kind != "current" and not dry_run:
        atomic_write(manifest_path, "".join(tables))
        atomic_write(lock_path, lock)
    return replacement


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("kind", choices=["current", "patch", "minor", "major", "custom"])
    parser.add_argument("--version", default="", help="X.Y.Z for the custom choice")
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    args = parser.parse_args()
    try:
        print(bump(args.root, args.kind, args.dry_run, args.version))
    except (OSError, ValueError, KeyError, tomllib.TOMLDecodeError) as error:
        print(f"release version: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
