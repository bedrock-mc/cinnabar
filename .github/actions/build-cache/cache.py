"""Cache compiler outputs and restore timestamps only for identical tracked inputs."""

import hashlib
import json
import os
from pathlib import Path
import stat
import subprocess
import sys


def command(*args, cwd=None):
    """Run a command and return its output, failing on unsuccessful execution."""
    return subprocess.check_output(args, cwd=cwd, text=True).strip()


def snapshot(root):
    """Record tracked files and directories containing only those tracked inputs."""
    names = command("git", "ls-files", "-z", cwd=root).split("\0")
    files = {}
    for name in names:
        path = root / name
        if not name or not path.exists() or path.is_symlink():
            continue
        info = path.stat()
        if stat.S_ISREG(info.st_mode):
            files[name] = {
                "kind": "file",
                "hash": hashlib.sha256(path.read_bytes()).hexdigest(),
                "mode": stat.S_IMODE(info.st_mode),
                "mtime": info.st_mtime_ns,
            }
    directories = {parent for name in files for parent in Path(name).parents if parent != Path(".")}
    for directory in sorted(directories, key=lambda path: len(path.parts), reverse=True):
        path = root / directory
        children = sorted(child.relative_to(root).as_posix() for child in path.iterdir())
        # Never hide new, ignored, or symlinked inputs from a directory-watching build script.
        if any(child not in files for child in children):
            continue
        contents = [(child, files[child]["hash"], files[child]["mode"], files[child]["kind"])
                    for child in children]
        info = path.stat()
        files[directory.as_posix()] = {
            "kind": "directory",
            "hash": hashlib.sha256(json.dumps(contents).encode()).hexdigest(),
            "mode": stat.S_IMODE(info.st_mode),
            "mtime": info.st_mtime_ns,
        }
    return files


def same_content(left, right):
    """Compare the contents and permissions, independently of checkout timestamps."""
    return (left["hash"], left["mode"], left["kind"]) == (right["hash"], right["mode"], right["kind"])


def restore_inputs(root):
    """Reuse saved timestamps for unchanged files and record this build's inputs."""
    manifest = root / "target/.ci-inputs.json"
    try:
        previous = json.loads(manifest.read_text())
    except (FileNotFoundError, json.JSONDecodeError):
        previous = {}
    current = snapshot(root)
    restored = 0
    for name, info in current.items():
        path = root / name
        old = previous.get(name)
        if old and same_content(info, old):
            os.utime(path, ns=(path.stat().st_atime_ns, old["mtime"]))
            info["mtime"] = old["mtime"]
            restored += 1
        else:
            # A concurrent cache writer may have built after this job checked out.
            # Changed inputs must be newer than the artifacts we just restored.
            os.utime(path, None)
            info["mtime"] = path.stat().st_mtime_ns
    manifest.parent.mkdir(parents=True, exist_ok=True)
    manifest.write_text(json.dumps(current, sort_keys=True))
    print(f"Reused timestamps for {restored}/{len(current)} unchanged source inputs")


def verify_inputs(root):
    """Refuse to save artifacts if a build changed the tracked source inputs."""
    before = json.loads((root / "target/.ci-inputs.json").read_text())
    after = snapshot(root)
    if before.keys() != after.keys() or any(
        not same_content(before[name], after[name]) for name in before
    ):
        raise RuntimeError("Tracked sources changed during the build; refusing to cache outputs")


def plan(root, language, lane, env):
    """Separate incompatible compilers and profiles, with a new save key per commit."""
    bucket = f"build-v1-{language}-{env['RUNNER_OS']}-{env['RUNNER_ARCH']}-{lane}-"
    if language == "rust":
        target = env.get("CARGO_BUILD_TARGET", "")
        if target:
            # Keep each cross target's outputs and retention independent on the same host.
            bucket += target + "-"
        compiler = command("rustc", "-vV", cwd=root)
        settings = {name: value for name, value in env.items() if name.startswith(
            ("CARGO_", "RUST", "CC", "CXX", "CFLAGS", "CPPFLAGS", "LDFLAGS", "CMAKE_")
        )}
        profile = "release" if lane == "release" else "debug"
        cargo_home = Path(env.get("CARGO_HOME", Path.home() / ".cargo"))
        # Cargo treats registry packages as immutable. Only native build scripts
        # need extracted sources preserved; other sources can be unpacked again.
        paths = [root / "target" / profile, root / "target/.ci-inputs.json",
                 cargo_home / "registry/index", cargo_home / "registry/cache",
                 cargo_home / "registry/src/*/*-sys-*", cargo_home / "git"]
        if target:
            # The first path keeps native build scripts and proc-macros too.
            paths.append(root / "target" / target / profile)
    elif language == "go":
        if env.get("GOOS") and env.get("GOARCH"):
            bucket += f"{env['GOOS']}-{env['GOARCH']}-"
        compiler = command("go", "version", cwd=root)
        settings = {name: value for name, value in env.items() if name.startswith(
            ("GO", "CGO_", "CC", "CXX")
        )}
        paths = command("go", "env", "GOCACHE", "GOMODCACHE", cwd=root).splitlines()
    else:
        raise ValueError(f"Unknown cache language: {language}")
    settings["runner-image"] = env.get("ImageOS", "")
    identity = hashlib.sha256((compiler + json.dumps(settings, sort_keys=True)).encode()).hexdigest()[:16]
    prefix = bucket + identity + "-"
    revision = command("git", "rev-parse", "HEAD", cwd=root)
    attempt = f"{env.get('GITHUB_RUN_ID', 'local')}-{env.get('GITHUB_RUN_ATTEMPT', '1')}"
    return {"bucket": bucket, "prefix": prefix, "key": prefix + revision + "-" + attempt,
            "paths": "\n".join(str(path) for path in paths)}


def superseded(caches, key, bucket, ref):
    """Select only older entries after confirming the replacement exists on this ref."""
    replacement = next((entry for entry in caches if entry["key"] == key and entry["ref"] == ref), None)
    if replacement is None:
        return []
    return [entry["id"] for entry in caches if entry["ref"] == ref
            and entry["key"].startswith(bucket) and entry["key"] != key
            and entry["created_at"] < replacement["created_at"]]


def prune(env):
    """Remove superseded entries only after a successful save, preserving concurrent newer saves."""
    repo = env["GITHUB_REPOSITORY"]
    pages = json.loads(command("gh", "api", "--paginate", "--slurp",
                              f"repos/{repo}/actions/caches?per_page=100"))
    caches = [entry for page in pages for entry in page["actions_caches"]]
    for cache_id in superseded(caches, env["CACHE_KEY"], env["CACHE_BUCKET"], env["GITHUB_REF"]):
        command("gh", "api", "--method", "DELETE", f"repos/{repo}/actions/caches/{cache_id}")


def main():
    """Dispatch the action's planning, source verification, and retention steps."""
    env = os.environ
    root = Path(env.get("CACHE_WORKSPACE", ".")).resolve()
    action = sys.argv[1]
    if action == "plan":
        outputs = plan(root, env["CACHE_LANGUAGE"], env["CACHE_LANE"], env)
        with open(env["GITHUB_OUTPUT"], "a") as output:
            for name, value in outputs.items():
                output.write(f"{name}<<CACHE_OUTPUT\n{value}\nCACHE_OUTPUT\n")
        if env["CACHE_LANGUAGE"] == "rust":
            with open(env["GITHUB_ENV"], "a") as output:
                output.write("CARGO_INCREMENTAL=0\n")
    elif action == "restore-inputs":
        restore_inputs(root)
    elif action == "verify-inputs":
        verify_inputs(root)
    elif action == "prune":
        prune(env)
    else:
        raise ValueError(f"Unknown cache action: {action}")


if __name__ == "__main__":
    main()
