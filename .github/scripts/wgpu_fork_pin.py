"""Fail unless every wgpu fork patch pins one commit on the fork's cinnabar-v<major> branch."""

import json
import os
from pathlib import Path
import sys
import tomllib
import urllib.request

FORK_PREFIX = "https://github.com/"


def pin(manifest: dict) -> tuple[str, str, str]:
    """Returns the fork repository, the pinned commit and the release branch for the wgpu major."""
    major = manifest["workspace"]["dependencies"]["wgpu"]["version"].split(".")[0]
    patches = [
        entry
        for entry in manifest["patch"]["crates-io"].values()
        if isinstance(entry, dict) and "/wgpu" in entry.get("git", "")
    ]
    if not patches:
        raise ValueError("no wgpu fork patches in [patch.crates-io]")
    pins = {(entry["git"], entry.get("rev")) for entry in patches}
    if len(pins) != 1 or None in {rev for _, rev in pins}:
        raise ValueError(f"wgpu fork patches must share one git URL and rev, found {sorted(map(str, pins))}")
    (url, rev), = pins
    if not url.startswith(FORK_PREFIX):
        raise ValueError(f"unexpected wgpu fork URL {url}")
    return url.removeprefix(FORK_PREFIX).removesuffix(".git"), rev, f"cinnabar-v{major}"


def on_branch(status: str) -> bool:
    """Whether a GitHub compare of branch...commit means the commit is on the branch."""
    return status in ("identical", "behind")


def compare_status(repository: str, branch: str, rev: str) -> str:
    """Returns GitHub's compare status of the branch against the commit."""
    request = urllib.request.Request(f"https://api.github.com/repos/{repository}/compare/{branch}...{rev}")
    if token := os.environ.get("GH_TOKEN"):
        request.add_header("Authorization", f"Bearer {token}")
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.load(response)["status"]


def main() -> int:
    manifest = tomllib.loads(Path(sys.argv[1] if len(sys.argv) > 1 else "Cargo.toml").read_text())
    repository, rev, branch = pin(manifest)
    status = compare_status(repository, branch, rev)
    if not on_branch(status):
        print(f"{rev} is not on {repository}:{branch} ({status}); merge it there before pinning it.", file=sys.stderr)
        return 1
    print(f"wgpu fork pin {rev[:9]} is on {repository}:{branch}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
