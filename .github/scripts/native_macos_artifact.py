"""Build source-bound native macOS acceptance executables without packaging user data."""

import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tomllib


def capture(*args):
    """Read one tool's explicit output, propagating failure without an environment dump."""
    return subprocess.check_output(args, text=True).strip()


def sha256(path):
    """Hash a file without retaining its complete contents in memory."""
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def main():
    """Archive only the two actual compiler-selected executables and their build receipt."""
    root = Path.cwd().resolve()
    head = capture("git", "rev-parse", "HEAD")
    if head != os.environ["GITHUB_SHA"] or capture("git", "status", "--porcelain"):
        raise ValueError("the clean source checkout must match this workflow's commit")
    tree = capture("git", "rev-parse", "HEAD^{tree}")
    lock = sha256(root / "Cargo.lock")
    target = os.environ["CARGO_BUILD_TARGET"]
    output = Path(os.environ["RUNNER_TEMP"]) / "native-macos-artifact"
    binaries = output / "bin"
    binaries.mkdir(parents=True)
    packages = {}
    command = ["cargo", "build", "--locked", "--target", target]
    for relative in ("app/Cargo.toml", "tools/cinnabar-mcp/Cargo.toml"):
        manifest = root / relative
        source = tomllib.loads(manifest.read_text())
        [binary] = source["bin"]
        packages[manifest] = (source["package"]["name"], binary["name"])
        command += ["-p", source["package"]["name"], "--bin", binary["name"]]
    client = tomllib.loads((root / "app/Cargo.toml").read_text())
    features = ("native-networking", "developer-control")
    required = set(features) | set(client["features"]["default"])
    command += ["--features", ",".join(f"{client['package']['name']}/{name}" for name in features),
                "--message-format=json-render-diagnostics"]
    compiler = output / "compiler-artifacts.jsonl"
    with compiler.open("w") as stream:
        subprocess.run(command, stdout=stream, check=True,
                       env={**os.environ, "CINNABAR_BUILD_COMMIT": head})
    emitted = {}
    finished = False
    with compiler.open() as stream:
        for line in stream:
            event = json.loads(line)
            if event["reason"] == "build-finished":
                finished = event["success"]
            if event["reason"] != "compiler-artifact" or not event.get("executable"):
                continue
            manifest = Path(event["manifest_path"]).resolve()
            if manifest not in packages or "bin" not in event["target"]["kind"]:
                continue
            package, name = packages[manifest]
            if event["target"]["name"] != name or event["profile"]["test"] or name in emitted:
                raise ValueError("the build must emit each selected binary exactly once")
            if manifest == root / "app/Cargo.toml" and not required <= set(event["features"]):
                raise ValueError("the client artifact is missing its requested or default features")
            executable = Path(event["executable"]).resolve()
            architecture = capture("lipo", "-archs", str(executable))
            if architecture != "arm64":
                raise ValueError(f"unexpected native artifact architecture: {architecture}")
            build_version = capture("xcrun", "vtool", "-show-build", str(executable))
            destination = binaries / name
            shutil.copyfile(executable, destination)
            destination.chmod(0o755)
            if sha256(destination) != sha256(executable):
                raise ValueError("the staged executable differs from its compiler output")
            emitted[name] = {"package": package, "package_id": event["package_id"],
                             "features": event["features"], "path": f"bin/{name}",
                             "sha256": sha256(destination), "bytes": destination.stat().st_size,
                             "mach_o_architecture": architecture, "build_version": build_version}
    if not finished or set(emitted) != {name for _, name in packages.values()}:
        raise ValueError("the successful build must contain both original executable targets")
    if (capture("git", "rev-parse", "HEAD") != head
            or capture("git", "status", "--porcelain") or sha256(root / "Cargo.lock") != lock):
        raise ValueError("the build changed its source or locked dependency graph")
    receipt = {"schema": 1, "source_commit": head, "source_tree": tree, "lock_sha256": lock,
               "target": target, "command": command, "profile": "dev",
               "dev_debug": os.environ["CARGO_PROFILE_DEV_DEBUG"],
               "host_architecture": capture("uname", "-m"), "rustc": capture("rustc", "-vV"),
               "cargo": capture("cargo", "--version"), "macos": capture("sw_vers", "-productVersion"),
               "repository": os.environ["GITHUB_REPOSITORY"], "ref": os.environ["GITHUB_REF"],
               "workflow": os.environ["GITHUB_WORKFLOW_REF"],
               "workflow_commit": os.environ["GITHUB_WORKFLOW_SHA"],
               "run_id": os.environ["GITHUB_RUN_ID"], "attempt": os.environ["GITHUB_RUN_ATTEMPT"],
               "compiler_json_sha256": sha256(compiler), "executables": emitted}
    (output / "manifest.json").write_text(json.dumps(receipt, indent=2) + "\n")
    name = f"native-macos-{target}-{head}"
    archive = output.with_name(name + ".tar.gz")
    with tarfile.open(archive, "w:gz") as stream:
        stream.add(output, arcname="native-macos")
    with Path(os.environ["GITHUB_OUTPUT"]).open("a") as stream:
        stream.write(f"name={name}\narchive={archive}\n")


if __name__ == "__main__":
    main()
