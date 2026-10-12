"""Cache compiler outputs and restore timestamps only for identical tracked inputs."""

import glob
import hashlib
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
import sys
import tarfile
import zipfile


# GitHub evicts least recently used entries once a repository's caches exceed this, and the
# rarely used ones go first. Saved buckets must fit together.
CACHE_LIMIT = 10_000_000_000
BUDGETS = Path(__file__).resolve().with_name("budgets.json")


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
    target = ""
    if language == "rust":
        target = env.get("CARGO_BUILD_TARGET", "")
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
            target = f"{env['GOOS']}-{env['GOARCH']}"
        compiler = command("go", "version", cwd=root)
        settings = {name: value for name, value in env.items() if name.startswith(
            ("GO", "CGO_", "CC", "CXX")
        )}
        paths = command("go", "env", "GOCACHE", "GOMODCACHE", cwd=root).splitlines()
    else:
        raise ValueError(f"Unknown cache language: {language}")
    # Put the target before the lane so native buckets cannot prune cross-target caches.
    platform = f"build-v1-{language}-{env['RUNNER_OS']}-{env['RUNNER_ARCH']}-"
    bucket = platform + (target + "-" if target else "") + lane + "-"
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


class BudgetError(RuntimeError):
    """A save that would break the cache budgets, reported as a workflow error annotation."""


def budget(store, bucket):
    """Return the byte budget for a bucket, refusing buckets nobody has budgeted."""
    budgets = json.loads(BUDGETS.read_text())[store]
    if bucket not in budgets:
        raise BudgetError(f"{bucket} has no {store} budget in {BUDGETS.name}")
    return budgets[bucket]


def caches(env):
    """List the repository's cache entries."""
    pages = json.loads(command("gh", "api", "--paginate", "--slurp",
                              f"repos/{env['GITHUB_REPOSITORY']}/actions/caches?per_page=100"))
    return [entry for page in pages for entry in page["actions_caches"]]


def delete(endpoint):
    """Delete an API resource; another job deleting it first is not an error."""
    result = subprocess.run(["gh", "api", "--method", "DELETE", endpoint],
                            capture_output=True, text=True)
    if result.returncode != 0 and "HTTP 404" not in result.stderr:
        raise subprocess.CalledProcessError(result.returncode, result.args, result.stdout, result.stderr)


def delete_caches(env, ids):
    """Delete cache entries by id."""
    for cache_id in ids:
        delete(f"repos/{env['GITHUB_REPOSITORY']}/actions/caches/{cache_id}")


def replaced(entries, bucket, ref):
    """Select the entries a save on this ref replaces, so old and new never coexist."""
    return [entry["id"] for entry in entries if entry["ref"] == ref and entry["key"].startswith(bucket)]


def make_room(env):
    """Check the bucket is budgeted, then delete its entries before the replacement is saved.

    Keeping both until the save finished would briefly exceed the limit and evict another
    bucket. Jobs restoring this bucket during the upload build without it.
    """
    budget("cache", env["CACHE_BUCKET"])
    delete_caches(env, replaced(caches(env), env["CACHE_BUCKET"], env["GITHUB_REF"]))


def prune(env):
    """Remove superseded entries after a save, and reject a saved entry over its budget."""
    entries = caches(env)
    delete_caches(env, superseded(entries, env["CACHE_KEY"], env["CACHE_BUCKET"], env["GITHUB_REF"]))
    saved = next((entry for entry in entries if entry["key"] == env["CACHE_KEY"]), None)
    limit = budget("cache", env["CACHE_BUCKET"])
    if saved is not None and saved["size_in_bytes"] > limit:
        delete_caches(env, [saved["id"]])
        raise BudgetError(f"{env['CACHE_KEY']} is {saved['size_in_bytes']} bytes, over its "
                           f"{limit}-byte budget; raise the budget only if the total still fits")


def archive_roots(paths):
    """Pair each planned path with the directory it is archived relative to, and its matches."""
    for index, path in enumerate(paths):
        parts = Path(path).parts
        literal = next((i for i, part in enumerate(parts) if any(c in part for c in "*?[")), None)
        if literal is None:
            root, matches = Path(path).parent, [Path(path)] if Path(path).exists() else []
        else:
            root, matches = Path(*parts[:literal]), sorted(Path(match) for match in glob.glob(path))
        yield index, root, matches


def pack(paths, archive):
    """Archive planned paths with their exact timestamps, which Cargo freshness depends on."""
    with tarfile.open(archive, "w:gz", compresslevel=1, format=tarfile.PAX_FORMAT) as tar:
        for index, root, matches in archive_roots(paths):
            # Hard links may only point within their own root.
            tar.inodes.clear()
            for match in matches:
                tar.add(match, arcname=f"{index}/{match.relative_to(root).as_posix()}")


def cache_member(member, root):
    """Accept files, directories and hard links inside `root`, checked as the data filter does."""
    if not (member.isfile() or member.isdir() or member.islnk()):
        raise tarfile.SpecialFileError(member)
    return tarfile.data_filter(member, root)


def inside(root, name):
    """Return where an archive name lands under `root`, or None if it is absolute or escapes.

    Checked before extraction: tarfile's data filter strips a leading slash instead of refusing.
    """
    parts = name.split("/")
    if (not name or name.startswith("/") or "\\" in name or ":" in parts[0]
            or any(part == ".." for part in parts)):
        return None
    destination = root.joinpath(*(part for part in parts if part not in ("", ".")))
    real_root = os.path.realpath(root)
    real = os.path.realpath(destination)
    return destination if os.path.commonpath([real_root, real]) == real_root else None


def unpack(paths, stream):
    """Extract an archive from `pack` into this runner's planned locations.

    Every entry must name a planned root and stay inside it; anything else aborts the restore
    before it touches the file system.
    """
    roots = {str(index): root for index, root, _ in archive_roots(paths)}
    directories = []
    with tarfile.open(fileobj=stream, mode="r|gz") as tar:
        for member in tar:
            index, _, name = member.name.partition("/")
            root = roots.get(index)
            destination = root and inside(root, name)
            if not destination:
                raise tarfile.OutsideDestinationError(member, member.name)
            member.name = name
            if member.islnk():
                link_index, _, link_name = member.linkname.partition("/")
                if link_index != index or not inside(root, link_name):
                    raise tarfile.LinkOutsideDestinationError(member, member.linkname)
                member.linkname = link_name
            if member.isdir():
                # Read-only directories, such as Go's module cache, get their mode last.
                directories.append((destination, member.mode & 0o755, member.mtime))
                member.mode |= 0o700
            tar.extract(member, root, filter=cache_member)
    for path, mode, mtime in reversed(directories):
        os.chmod(path, mode)
        os.utime(path, (mtime, mtime))


# Events that run only this repository's own code; pull request runs can come from forks.
TRUSTED_EVENTS = {"push", "schedule", "workflow_dispatch"}


def trusted_run(run, env):
    """Whether this workflow saved the run's artifacts on this repository's default branch."""
    workflow = env["GITHUB_WORKFLOW_REF"].split("@")[0].split("/", 2)[2]
    repository = int(env["GITHUB_REPOSITORY_ID"])
    return (run["event"] in TRUSTED_EVENTS and run["head_branch"] == env["CACHE_BRANCH"]
            and run["path"] == workflow
            and (run.get("repository") or {}).get("id") == repository
            and (run.get("head_repository") or {}).get("id") == repository)


def trusted_artifacts(listed, run_of, env):
    """Keep unexpired artifacts whose run, looked up with `run_of`, is trusted; newest first."""
    found = [artifact for artifact in listed if not artifact["expired"]
             and (artifact.get("workflow_run") or {}).get("id") is not None
             and trusted_run(run_of(artifact["workflow_run"]["id"]), env)]
    return sorted(found, key=lambda artifact: artifact["created_at"], reverse=True)


def artifacts(env, name):
    """List this repository's trusted artifacts with this name, newest first."""
    repo = env["GITHUB_REPOSITORY"]
    listing = json.loads(command("gh", "api", f"repos/{repo}/actions/artifacts?name={name}&per_page=100"))
    return trusted_artifacts(
        listing["artifacts"],
        lambda run_id: json.loads(command("gh", "api", f"repos/{repo}/actions/runs/{run_id}")),
        env,
    )


def restore_artifact(env):
    """Restore the newest trusted artifact; a missing or broken one, or an API error, leaves a cold build."""
    paths = env["CACHE_PATHS"].splitlines()
    name = env["CACHE_PREFIX"].rstrip("-")
    download = Path(env["RUNNER_TEMP"]) / f"{name}.zip"
    try:
        found = artifacts(env, name)
        if not found:
            print(f"No trusted artifact named {name}; building without a cache")
            return
        with open(download, "wb") as output:
            subprocess.run(["gh", "api", f"repos/{env['GITHUB_REPOSITORY']}/actions/artifacts/"
                            f"{found[0]['id']}/zip"], stdout=output, check=True)
        with zipfile.ZipFile(download) as zipped, zipped.open(zipped.namelist()[0]) as stream:
            unpack(paths, stream)
        print(f"Restored artifact {found[0]['id']} ({found[0]['size_in_bytes']} bytes) for {name}")
    except (OSError, ValueError, KeyError, subprocess.CalledProcessError, tarfile.TarError,
            zipfile.BadZipFile) as error:
        # A partial extraction could pair fresh fingerprints with truncated outputs.
        for _, _, matches in archive_roots(paths):
            for match in matches:
                if match.is_dir():
                    shutil.rmtree(match, ignore_errors=True)
                else:
                    match.unlink(missing_ok=True)
        print(f"::warning::Discarded artifact cache {name}: {error}")
    finally:
        download.unlink(missing_ok=True)


def pack_artifact(env):
    """Archive the planned paths for upload, refusing an archive over its budget."""
    name = env["CACHE_PREFIX"].rstrip("-")
    archive = Path(env["RUNNER_TEMP"]) / f"{name}.tar.gz"
    pack(env["CACHE_PATHS"].splitlines(), archive)
    size, limit = archive.stat().st_size, budget("artifact", env["CACHE_BUCKET"])
    if size > limit:
        archive.unlink()
        raise BudgetError(f"{name} archive is {size} bytes, over its {limit}-byte budget")
    with open(env["GITHUB_OUTPUT"], "a") as output:
        output.write(f"archive={archive}\nname={name}\n")


def superseded_artifacts(found, keep):
    """Select artifacts older than the kept replacement, once it is listed; newer uploads from
    concurrent runs stay."""
    replacement = next((artifact for artifact in found if artifact["id"] == keep), None)
    if replacement is None:
        return []
    return [artifact["id"] for artifact in found
            if artifact["created_at"] < replacement["created_at"]]


def prune_artifacts(env):
    """Delete the uploaded archive, then artifacts of this name older than the new one."""
    Path(env["CACHE_ARCHIVE"]).unlink(missing_ok=True)
    name, keep = env["CACHE_PREFIX"].rstrip("-"), int(env["CACHE_ARTIFACT_ID"])
    for artifact_id in superseded_artifacts(artifacts(env, name), keep):
        delete(f"repos/{env['GITHUB_REPOSITORY']}/actions/artifacts/{artifact_id}")


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
    elif action == "make-room":
        make_room(env)
    elif action == "prune":
        prune(env)
    elif action == "restore-artifact":
        restore_artifact(env)
    elif action == "pack-artifact":
        pack_artifact(env)
    elif action == "prune-artifacts":
        prune_artifacts(env)
    else:
        raise ValueError(f"Unknown cache action: {action}")


if __name__ == "__main__":
    try:
        main()
    except BudgetError as error:
        print(f"::error title=Cache budget::{error}")
        sys.exit(1)
