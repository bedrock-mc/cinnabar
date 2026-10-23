//! One-command authoring on top of the existing SDK and client bundle builder.

mod scaffold;

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

/// Runs the repository's `mod new` and `mod build` commands.
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
        _ => bail!(
            "usage: mod new <name> [--sdk <experience-sdk path>] | mod build [--project <directory>] [--release]"
        ),
    }
}

/// Locates the SDK shipped with this repository's developer tool.
fn sdk_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/experience-sdk")
}

/// Builds the client and writes a package directory containing its `.cxb` and Go definitions.
pub fn build(root: &Path, release: bool) -> Result<PathBuf> {
    let root = root.canonicalize()?;
    let manifest = root.join("Cargo.toml");
    let cargo: toml::Value = toml::from_str(&fs::read_to_string(&manifest)?)?;
    let version = cargo
        .get("package")
        .and_then(|package| package.get("version"))
        .and_then(toml::Value::as_str)
        .context("mod package needs an explicit package.version")?;
    let mut command = Command::new("cargo");
    command.current_dir(&root).args([
        "build",
        "--lib",
        "--target",
        "wasm32-unknown-unknown",
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
                if event["manifest_path"].as_str().map(Path::new) == Some(manifest.as_path()) =>
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
        "mod compilation failed; the package was not published"
    );
    let (package, component) =
        artifact.context("Cargo produced no client .wasm; use a cdylib crate")?;
    let generated = scripts
        .get(&package)
        .context("mod build.rs did not generate shared definitions")?;
    let project = Project::parse(&fs::read_to_string(root.join("src/shared.rs"))?)?;
    let built_schema: Value = serde_json::from_slice(&fs::read(generated.join("registry.json"))?)?;
    ensure!(
        built_schema == project.document(),
        "shared definitions changed during compilation; rebuild the project"
    );
    let source = source(&project, version)?;
    let seed = root.join(".local/publisher.seed");
    fs::create_dir_all(seed.parent().context("publisher directory missing")?)?;
    if !seed.exists() {
        keys::generate(&seed)?;
    }
    let assets = bundle::read_assets(&root.join("assets"))?;
    let built = bundle::build(source, &fs::read(component)?, &assets, &keys::read(&seed)?)?;
    let destination = root.join("dist").join(&project.id);
    let staging = root
        .join("dist")
        .join(format!(".{}-{}", project.id, std::process::id()));
    fs::create_dir_all(staging.join("messages"))?;
    fs::write(staging.join(format!("{}.cxb", project.id)), built.bytes)?;
    fs::copy(
        generated.join("messages.go"),
        staging.join("messages/messages.go"),
    )?;
    fs::copy(
        generated.join("registry.json"),
        staging.join("registry.json"),
    )?;
    fs::write(
        staging.join("messages/go.mod"),
        experience_sdk::project::go_module(&project.id),
    )?;
    if destination.exists() {
        fs::remove_dir_all(&destination)?;
    }
    fs::rename(staging, &destination)?;
    writeln!(std::io::stdout(), "package={}", destination.display())?;
    Ok(destination)
}

/// Converts only generated metadata to the existing bundle source; host validation stays canonical.
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
