#!/usr/bin/env python3
"""Resolve the package workflow's exact source commit and optional release tag.

Environment: EVENT_NAME=push|schedule|workflow_dispatch, DEFAULT_BRANCH (main),
BUMP=current|patch|minor|major|custom|nightly, CUSTOM_VERSION=X.Y.Z, TARGET_BRANCH,
optional RELEASE_TAG for a tag push, and the
standard GITHUB_REF, GITHUB_SHA, GITHUB_REPOSITORY, GITHUB_OUTPUT, GH_TOKEN.
Dispatch must check out TARGET_BRANCH with full history and tags. A nightly
dispatch is read-only and limited to the default branch; a tagged
dispatch writes a release commit/tag; GitHub's token does not trigger another
tag workflow. Outputs: channel, track, tag, version, ref, commit, cargo_features.
ref is a commit SHA; cargo_features is a comma-separated list for every Cargo build.
"""

import os
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys
from urllib.parse import urlsplit


TOOLING = Path(__file__).resolve().parent
ROOT = Path(os.environ.get("CINNABAR_SOURCE_ROOT", TOOLING.parent)).resolve()
TAG = re.compile(r"v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)")


class ReleaseError(Exception):
    pass


def channel_for_track(track: str) -> tuple[str, dict]:
    """Selects the sole published channel for a release track from shared metadata."""
    channels = json.loads((TOOLING / "release-assets.json").read_text())["update_channels"]
    matches = [(name, release) for name, release in channels.items() if release["track"] == track]
    if len(matches) != 1:
        raise ReleaseError(f"release metadata must declare exactly one {track} channel")
    features = matches[0][1].get("cargo_features", [])
    if not isinstance(features, list) or any(
        not isinstance(feature, str) or not re.fullmatch(r"[A-Za-z0-9_][A-Za-z0-9_-]*", feature)
        for feature in features
    ) or len(set(features)) != len(features):
        raise ReleaseError(f"{track} cargo_features must be unique Cargo feature names")
    return matches[0]


def command(*args: str, check: bool = True) -> subprocess.CompletedProcess:
    executable = shutil.which(args[0])
    if executable is None:
        raise ReleaseError(f"required command is unavailable: {args[0]}")
    args = (executable, *args[1:])
    result = subprocess.run(args, cwd=ROOT, text=True, capture_output=True)
    if check and result.returncode:
        raise ReleaseError(f"{' '.join(args[:3])} failed: {result.stderr.strip()}")
    return result


def git(*args: str, check: bool = True) -> str:
    return command("git", *args, check=check).stdout.strip()


def version(kind: str = "current", dry_run: bool = False) -> str:
    args = [sys.executable, str(TOOLING / "bump-version.py"), kind, "--root", str(ROOT)]
    if kind == "custom":
        args.extend(["--version", os.environ.get("CUSTOM_VERSION", "")])
    if dry_run:
        args.append("--dry-run")
    return command(*args).stdout.strip()


def release_exists(repository: str, tag: str) -> bool:
    # An authentication/network error must not be interpreted as a missing release.
    result = command("gh", "api", "--include", f"repos/{repository}/releases/tags/{tag}", check=False)
    if result.returncode == 0:
        return True
    status = re.search(r"(?m)^HTTP/\S+\s+(\d{3})\b", result.stdout)
    if (status and status[1] == "404") or "(HTTP 404)" in result.stderr:
        return False
    raise ReleaseError(f"cannot verify whether release {tag} exists: {result.stderr.strip()}")


def tag_state(tag: str) -> tuple[str | None, str | None]:
    local = command("git", "rev-parse", "--verify", f"refs/tags/{tag}^{{commit}}", check=False)
    local_commit = local.stdout.strip() if local.returncode == 0 else None
    remote = git("ls-remote", "--tags", "origin", f"refs/tags/{tag}", f"refs/tags/{tag}^{{}}")
    refs = dict(line.split()[::-1] for line in remote.splitlines())
    remote_commit = refs.get(f"refs/tags/{tag}^{{}}", refs.get(f"refs/tags/{tag}"))
    return local_commit, remote_commit


def configure_identity() -> None:
    git("config", "user.name", "github-actions[bot]")
    git("config", "user.email", "41898282+github-actions[bot]@users.noreply.github.com")


def dispatch(target_branch: str, bump: str) -> tuple[str, str]:
    repository = os.environ.get("GITHUB_REPOSITORY", "")
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository):
        raise ReleaseError("GITHUB_REPOSITORY must identify the release repository")
    if git("symbolic-ref", "--quiet", "--short", "HEAD") != target_branch:
        raise ReleaseError("manual releases must check out the selected target branch")
    current = version()
    head = git("rev-parse", "HEAD")
    current_tag = f"v{current}"
    states = tag_state(current_tag)
    retry_bump = bump in {"patch", "minor", "major"} or (
        bump == "custom" and os.environ.get("CUSTOM_VERSION", "") == current
    )
    recover = retry_bump and head in states and not release_exists(repository, current_tag)
    if bump == "current" or recover:
        target = current
        tag = current_tag
        if release_exists(repository, tag):
            raise ReleaseError(f"release {tag} already exists; choose a version bump")
        if any(commit is not None and commit != head for commit in states):
            raise ReleaseError(f"tag {tag} is on another commit")
        remote_exists = states[1] is not None
        if states[0] is None and not remote_exists:
            configure_identity()
            git("tag", "-a", tag, "-m", f"Cinnabar {tag}")
    else:
        target = version(bump, dry_run=True)
        tag = f"v{target}"
        if release_exists(repository, tag):
            raise ReleaseError(f"release {tag} already exists")
        if any(commit is not None for commit in tag_state(tag)):
            raise ReleaseError(f"tag {tag} already exists")
        configure_identity()
        if version(bump) != target:
            raise ReleaseError("version changed while preparing the release")
        git("add", "--", "Cargo.toml", "Cargo.lock")
        git("commit", "-m", f"chore: release {tag}")
        head = git("rev-parse", "HEAD")
        git("tag", "-a", tag, "-m", f"Cinnabar {tag}")
        remote_exists = False
    refs = [f"HEAD:refs/heads/{target_branch}"]
    if not remote_exists:
        refs.append(f"refs/tags/{tag}:refs/tags/{tag}")
    git("push", "--atomic", "origin", *refs)
    return tag, target


def validate_api_origin() -> None:
    """Reject placeholder or malformed API origins before any release refs are changed."""
    contract_path = ROOT / "crates/bridge/data/contract.json"
    if not contract_path.exists():
        if (ROOT / "app/Cargo.toml").exists():
            raise ReleaseError("a client release requires the bridge contract: crates/bridge/data/contract.json")
        # Early targets without the client have no API origin to validate.
        return
    contract = json.loads(contract_path.read_text())
    origins = [row["value"] for row in contract["constants"] if row.get("go_name") == "CinnabarAPIBaseURL"]
    if not origins:
        # Older source revisions do not connect to the API and have no origin to validate.
        return
    if len(origins) != 1:
        raise ReleaseError("the bridge contract must declare one Cinnabar API origin")
    origin = urlsplit(origins[0])
    host = (origin.hostname or "").lower().rstrip(".")
    if host == "invalid" or host.endswith(".invalid"):
        raise ReleaseError("replace the placeholder Cinnabar API origin before packaging")
    if origin.scheme != "https" or not host or origin.username or origin.password or origin.query or origin.fragment or origin.path not in {"", "/"}:
        raise ReleaseError("the Cinnabar API origin must be an HTTPS origin")


def prepare() -> dict[str, str]:
    validate_api_origin()
    tagged_channel, tagged_release = channel_for_track("tagged")
    rolling_channel, rolling_release = channel_for_track("rolling")
    event = os.environ.get("EVENT_NAME", os.environ.get("GITHUB_EVENT_NAME", ""))
    default_branch = os.environ.get("DEFAULT_BRANCH", "main")
    git("check-ref-format", f"refs/heads/{default_branch}")
    if git("status", "--porcelain"):
        raise ReleaseError("release preparation requires a clean checkout")
    if event == "workflow_dispatch":
        target_branch = os.environ.get("TARGET_BRANCH", "") or default_branch
        git("check-ref-format", f"refs/heads/{target_branch}")
        bump = os.environ.get("BUMP", "current")
        if bump not in {"current", "patch", "minor", "major", "custom", rolling_channel}:
            raise ReleaseError("BUMP must be current, patch, minor, major, custom, or " + rolling_channel)
        if bump == rolling_channel:
            if target_branch != default_branch:
                raise ReleaseError(f"{bump} builds only package {default_branch}")
            if git("symbolic-ref", "--quiet", "--short", "HEAD") != default_branch:
                raise ReleaseError(f"manual {bump} builds must check out the default branch")
            # Rolling channels replace one fixed prerelease named after the channel.
            tag, source_version, channel = rolling_release["tag"], version(), rolling_channel
        else:
            tag, source_version = dispatch(target_branch, bump)
            channel = tagged_channel
    elif event in {"push", "schedule"}:
        ref = os.environ.get("GITHUB_REF", "")
        source_version = version()
        if ref == f"refs/heads/{default_branch}":
            tag, channel = rolling_release["tag"], rolling_channel
        elif ref.startswith("refs/tags/"):
            tag = os.environ.get("RELEASE_TAG", ref.removeprefix("refs/tags/"))
            if not TAG.fullmatch(tag) or ref != f"refs/tags/{tag}":
                raise ReleaseError("release tag must look like v1.2.3 and match the pushed ref")
            if tag.removeprefix("v") != source_version:
                raise ReleaseError(f"tag {tag} does not match source version {source_version}")
            if git("rev-parse", f"refs/tags/{tag}^{{commit}}") != git("rev-parse", "HEAD"):
                raise ReleaseError(f"tag {tag} is on another commit")
            channel = tagged_channel
        else:
            raise ReleaseError("only the default branch and v* tag pushes can package releases")
        event_sha = os.environ.get("GITHUB_SHA", "")
        if event_sha:
            if not re.fullmatch(r"[0-9a-f]{40}|[0-9a-f]{64}", event_sha):
                raise ReleaseError("GITHUB_SHA is not a commit hash")
            if git("rev-parse", f"{event_sha}^{{commit}}") != git("rev-parse", "HEAD"):
                raise ReleaseError("checkout does not match the pushed commit")
    else:
        raise ReleaseError("EVENT_NAME must be push, schedule or workflow_dispatch")
    commit = git("rev-parse", "HEAD")
    track = "tagged" if channel == tagged_channel else "rolling"
    release = tagged_release if track == "tagged" else rolling_release
    return {"channel": channel, "track": track, "tag": tag, "version": source_version,
            "ref": commit, "commit": commit,
            "cargo_features": ",".join(release.get("cargo_features", []))}


def main() -> int:
    try:
        output = os.environ.get("GITHUB_OUTPUT")
        if not output:
            raise ReleaseError("GITHUB_OUTPUT must be set before release preparation")
        values = prepare()
        with open(output, "a") as destination:
            for name, value in values.items():
                destination.write(f"{name}={value}\n")
        print(f"Prepared {values['channel']} {values['tag']} at {values['commit']}")
        return 0
    except (OSError, ReleaseError) as error:
        print(f"release preparation: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
