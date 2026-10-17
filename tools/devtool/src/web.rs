//! Browser checks and reproducible static packaging; asset identities stay with their owner.

use crate::DevtoolError;
use serde::Deserialize;
use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

const WASM_TARGET: &str = "wasm32-unknown-unknown";
const JS_TESTS: [&str; 2] = [
    "web/transport/src/adapters.test.mjs",
    "web/site/edges.test.mjs",
];
type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// Checks the actual browser target and both JavaScript edge suites.
pub(crate) fn check(args: &[String]) -> std::result::Result<(), DevtoolError> {
    let checks = || -> Result<()> {
        if !args.is_empty() {
            return Err("check-web takes no arguments".into());
        }
        execute(Command::new("node").arg("--test").args(JS_TESTS))?;
        execute(Command::new("cargo").args([
            "check",
            "--locked",
            "--lib",
            "--target",
            WASM_TARGET,
            "-p",
            "web-client",
            "-p",
            "web-transport",
        ]))
    };
    checks().map_err(|error| DevtoolError::Web(error.to_string()))
}

/// Builds into a fresh output directory, leaving previous bundles intact on failure.
pub(crate) fn run(args: &[String]) -> std::result::Result<(), DevtoolError> {
    build(args).map_err(|error| DevtoolError::Web(error.to_string()))
}

/// Parses explicit asset/output paths and publishes a complete staged bundle.
fn build(args: &[String]) -> Result<()> {
    let mut assets = None;
    let mut output = None;
    let mut release = false;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--assets" => assets = args.next().map(PathBuf::from),
            "--out" => output = args.next().map(PathBuf::from),
            "--release" => release = true,
            _ => return Err(format!("unknown build-web argument: {arg}").into()),
        }
    }
    let (Some(assets), Some(output)) = (assets, output) else {
        return Err(
            "build-web --assets <compiled-assets-directory> --out <new-directory> [--release]"
                .into(),
        );
    };
    if output.exists() {
        return Err("output directory already exists; choose a new directory".into());
    }
    let version = bindgen_version(&fs::read_to_string("Cargo.lock")?)?;
    let metadata: serde_json::Value =
        serde_json::from_slice(&capture(Command::new("cargo").args([
            "metadata",
            "--locked",
            "--no-deps",
            "--format-version",
            "1",
        ]))?)?;
    let target = PathBuf::from(
        metadata["target_directory"]
            .as_str()
            .ok_or("missing Cargo target directory")?,
    );
    let bindgen = bindgen_cli(&target, &version)?;
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    if parent
        .canonicalize()?
        .starts_with(Path::new("web/site").canonicalize()?)
    {
        return Err("bundle output must be outside web/site".into());
    }
    let stage = tempfile::Builder::new()
        .prefix(".web-bundle-")
        .tempdir_in(parent)?;
    copy_site(Path::new("web/site"), stage.path())?;
    let manifest = capture(
        Command::new("cargo")
            .args([
                "run",
                "--locked",
                "-p",
                "web-client",
                "--bin",
                "manifest",
                "--",
            ])
            .arg(assets)
            .arg("--copy-to")
            .arg(stage.path().join("assets")),
    )?;
    fs::write(stage.path().join("assets/manifest.json"), manifest)?;
    let wasm_target = target.join("web");
    let mut cargo = Command::new("cargo");
    cargo
        .args([
            "build",
            "--locked",
            "--lib",
            "-p",
            "web-client",
            "--target",
            WASM_TARGET,
            "--target-dir",
        ])
        .arg(&wasm_target);
    if release {
        cargo.arg("--release");
    }
    execute(&mut cargo)?;
    execute(
        Command::new(bindgen)
            .args(["--target", "web", "--out-dir"])
            .arg(stage.path().join("pkg"))
            .arg(
                wasm_target
                    .join(WASM_TARGET)
                    .join(if release { "release" } else { "debug" })
                    .join("web_client.wasm"),
            ),
    )?;
    // All work succeeded; rename within the destination filesystem publishes it together.
    fs::rename(stage.path(), &output)?;
    println!("{}", output.display());
    Ok(())
}

/// Resolves the one binding version from the lock used for the wasm build.
fn bindgen_version(lock: &str) -> Result<String> {
    #[derive(Deserialize)]
    struct Lock {
        package: Vec<Package>,
    }
    #[derive(Deserialize)]
    struct Package {
        name: String,
        version: String,
    }
    let lock: Lock = toml::from_str(lock)?;
    let mut versions = lock
        .package
        .into_iter()
        .filter(|package| package.name == "wasm-bindgen");
    let version = versions
        .next()
        .ok_or("wasm-bindgen is missing from Cargo.lock")?
        .version;
    if versions.next().is_some() {
        return Err("multiple wasm-bindgen versions in Cargo.lock".into());
    }
    Ok(version)
}

/// Reuses an exact matching CLI, or installs it into this worktree's target directory.
fn bindgen_cli(target: &Path, version: &str) -> Result<PathBuf> {
    let matches = |program: &Path| {
        Command::new(program)
            .arg("--version")
            .output()
            .is_ok_and(|output| {
                output.status.success()
                    && String::from_utf8_lossy(&output.stdout).trim()
                        == format!("wasm-bindgen {version}")
            })
    };
    let global = PathBuf::from("wasm-bindgen");
    if matches(&global) {
        return Ok(global);
    }
    let root = target.join("web-tools").join(version);
    let binary = root
        .join("bin")
        .join(format!("wasm-bindgen{}", std::env::consts::EXE_SUFFIX));
    if !matches(&binary) {
        execute(
            Command::new("cargo")
                .args(["install", "--locked", "wasm-bindgen-cli", "--version"])
                .arg(format!("={version}"))
                .arg("--root")
                .arg(&root),
        )?;
    }
    if !matches(&binary) {
        return Err("installed wasm-bindgen CLI version does not match Cargo.lock".into());
    }
    Ok(binary)
}

/// Copies production site files recursively, excluding tests and earlier generated bindings.
fn copy_site(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == "pkg" || name.to_string_lossy().ends_with(".test.mjs") {
            continue;
        }
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy_site(&entry.path(), &destination.join(name))?;
        } else if kind.is_file() {
            fs::copy(entry.path(), destination.join(name))?;
        } else {
            return Err("site inputs must be regular files or directories".into());
        }
    }
    Ok(())
}

/// Runs a build step with live diagnostics and requires a successful exit.
fn execute(command: &mut Command) -> Result<()> {
    let status = command.status()?;
    if !status.success() {
        return Err(format!("{command:?} failed: {status}").into());
    }
    Ok(())
}

/// Captures machine-readable output while preserving a failed command's diagnostics.
fn capture(command: &mut Command) -> Result<Vec<u8>> {
    let output = command.output()?;
    if !output.status.success() {
        return Err(format!(
            "{command:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(output.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binding_version_requires_one_locked_package() {
        assert_eq!(
            bindgen_version("[[package]]\nname='wasm-bindgen'\nversion='1.2.3'").unwrap(),
            "1.2.3"
        );
        assert!(bindgen_version("package=[]").is_err());
        assert!(bindgen_version("[[package]]\nname='wasm-bindgen'\nversion='1'\n[[package]]\nname='wasm-bindgen'\nversion='2'").is_err());
    }
}
