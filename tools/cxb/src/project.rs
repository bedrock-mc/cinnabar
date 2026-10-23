//! One-command authoring on top of the existing SDK and client bundle builder.

mod local;
mod package;
mod scaffold;
pub use package::{install, unpack};

use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail, ensure};
use experience_sdk::project::Project;
use serde_json::Value;

use crate::{bundle, keys};
pub use scaffold::create;

/// Runs paired mod authoring, packaging, installation and development commands.
pub fn run(args: &[String]) -> Result<()> {
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["new", name] => create(Path::new(name), name, &sdk_path()),
        ["new", name, "--sdk", sdk] => create(Path::new(name), name, Path::new(sdk)),
        ["build"] => build(Path::new("."), false).map(|_| ()),
        ["build", "--release"] => build(Path::new("."), true).map(|_| ()),
        ["build", "--project", root] => build(Path::new(root), false).map(|_| ()),
        ["build", "--project", root, "--release"] => build(Path::new(root), true).map(|_| ()),
        ["install", package, "--server", world] => {
            install(Path::new(package), Path::new(world)).map(|_| ())
        }
        ["run"] => local::run(Path::new("."), false),
        ["run", "--project", root] => local::run(Path::new(root), false),
        ["run", "--project", root, "--release"] => local::run(Path::new(root), true),
        _ => bail!(
            "usage: mod new <name> [--sdk <experience-sdk path>] | mod build [--project <directory>] [--release] | mod install <file.cmod> --server <world> | mod run [--project <directory>] [--release]"
        ),
    }
}

/// Locates the SDK shipped with this repository's developer tool.
fn sdk_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/experience-sdk")
}

/// Builds both isolated components and publishes one installation archive.
pub fn build(root: &Path, release: bool) -> Result<PathBuf> {
    let root = root.canonicalize()?;
    let version = package_version(&root)?;
    let project = Project::parse(&fs::read_to_string(root.join("src/shared.rs"))?)?;
    let assets = bundle::read_assets(&root.join("assets"))?;
    let (client, generated) = compile(&root, release, "client")?;
    let (server, server_generated) = compile(&root, release, "server")?;
    for directory in [&generated, &server_generated] {
        let schema: Value = serde_json::from_slice(&fs::read(directory.join("registry.json"))?)?;
        ensure!(
            schema == project.document(),
            "shared definitions changed during compilation; rebuild"
        );
    }
    let seed = root.join(".local/publisher.seed");
    fs::create_dir_all(seed.parent().context("publisher directory missing")?)?;
    if !seed.exists() {
        keys::generate(&seed)?;
    }
    ensure!(
        assets == bundle::read_assets(&root.join("assets"))?,
        "mod assets changed during compilation; rebuild"
    );
    let client_assets = assets
        .iter()
        .filter(|(path, _)| !path.starts_with("server/"))
        .cloned()
        .collect::<Vec<_>>();
    let built = bundle::build(
        source(&project, &version)?,
        &client,
        &client_assets,
        &keys::read(&seed)?,
    )?;
    package::assemble(
        &root,
        &project,
        &version,
        &server,
        &assets,
        &built.bytes,
        &generated,
    )
}

/// Resolves inherited workspace versions through Cargo's own manifest reader.
fn package_version(root: &Path) -> Result<String> {
    let manifest = root.join("Cargo.toml");
    let output = Command::new("cargo")
        .current_dir(root)
        .args([
            "metadata",
            "--no-deps",
            "--format-version=1",
            "--manifest-path",
        ])
        .arg(&manifest)
        .output()?;
    std::io::stderr().write_all(&output.stderr)?;
    ensure!(output.status.success(), "reading mod Cargo metadata failed");
    let metadata: Value = serde_json::from_slice(&output.stdout)?;
    metadata["packages"]
        .as_array()
        .context("Cargo metadata packages missing")?
        .iter()
        .find(|package| {
            package["manifest_path"]
                .as_str()
                .is_some_and(|reported| is_manifest(reported, &manifest))
        })
        .and_then(|package| package["version"].as_str())
        .map(str::to_owned)
        .context("Cargo metadata did not identify the mod package")
}

/// Compiles exactly one half with defaults disabled and returns its own build-script output.
fn compile(root: &Path, release: bool, half: &str) -> Result<(Vec<u8>, PathBuf)> {
    let manifest = root.join("Cargo.toml");
    let mut command = Command::new("cargo");
    command.current_dir(root).args([
        "build",
        "--lib",
        "--target",
        "wasm32-unknown-unknown",
        "--no-default-features",
        "--features",
        half,
        "--message-format=json-render-diagnostics",
    ]);
    if release {
        command.arg("--release");
    }
    let output = command
        .output()
        .context("running Cargo; install the wasm32-unknown-unknown Rust target")?;
    std::io::stderr().write_all(&output.stderr)?;
    let mut scripts = BTreeMap::new();
    let mut artifact = None;
    for line in output
        .stdout
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        let event: Value = serde_json::from_slice(line).context("reading Cargo build output")?;
        match event["reason"].as_str() {
            Some("compiler-message") => {
                if let Some(rendered) = event["message"]["rendered"].as_str() {
                    std::io::stderr().write_all(rendered.as_bytes())?;
                }
            }
            Some("build-script-executed") => {
                if let (Some(package), Some(out)) =
                    (event["package_id"].as_str(), event["out_dir"].as_str())
                {
                    scripts.insert(package.to_owned(), PathBuf::from(out));
                }
            }
            Some("compiler-artifact")
                if event["manifest_path"]
                    .as_str()
                    .is_some_and(|reported| is_manifest(reported, &manifest)) =>
            {
                if let Some(files) = event["filenames"].as_array() {
                    for file in files
                        .iter()
                        .filter_map(Value::as_str)
                        .filter(|file| file.ends_with(".wasm"))
                    {
                        artifact = Some((
                            event["package_id"]
                                .as_str()
                                .context("Cargo artifact package missing")?
                                .to_owned(),
                            PathBuf::from(file),
                        ));
                    }
                }
            }
            _ => {}
        }
    }
    ensure!(
        output.status.success(),
        "{half} compilation failed; the package was not published"
    );
    let (package, component) =
        artifact.with_context(|| format!("Cargo produced no {half} .wasm; use a cdylib crate"))?;
    let generated = scripts
        .remove(&package)
        .context("mod build.rs did not generate shared definitions")?;
    Ok((fs::read(component)?, generated))
}

/// Whether Cargo's reported manifest path names `manifest`, which is already canonical.
/// Windows canonical paths are verbatim (`\\?\C:\...`) while Cargo reports plain drive paths.
fn is_manifest(reported: &str, manifest: &Path) -> bool {
    Path::new(reported)
        .canonicalize()
        .is_ok_and(|reported| reported == manifest)
}

/// Converts generated metadata to the existing bundle source; host validation stays canonical.
pub fn source(project: &Project, version: &str) -> Result<bundle::Source> {
    let channels = project
        .channels()
        .into_iter()
        .map(serde_json::from_value)
        .collect::<Result<Vec<_>, _>>()?;
    let permissions = project
        .permissions
        .iter()
        .map(|permission| serde_json::from_value(Value::String(permission.clone())))
        .collect::<Result<_, _>>()?;
    Ok(bundle::Source {
        id: project.id.clone(),
        package_version: version.into(),
        channels,
        permissions,
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn resolves_the_reference_projects_inherited_version() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/experiences/terminal-client")
            .canonicalize()
            .unwrap();
        assert_eq!(
            super::package_version(&root).unwrap(),
            env!("CARGO_PKG_VERSION")
        );
    }

    use super::is_manifest;

    /// A reported path that isn't spelled canonically still names the project manifest.
    #[test]
    fn non_canonical_reported_manifest_paths_match() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("Cargo.toml"), "").unwrap();
        let manifest = root.path().canonicalize().unwrap().join("Cargo.toml");
        let reported = root.path().join(".").join("Cargo.toml");
        assert!(is_manifest(reported.to_str().unwrap(), &manifest));
        assert!(!is_manifest(
            root.path().join("Other.toml").to_str().unwrap(),
            &manifest
        ));
    }
}
