//! Builds shared Rust definitions, channels, Go codecs and asset constants from one source.
#![expect(
    clippy::print_stdout,
    reason = "build scripts report Cargo dependency tracking directives"
)]

mod assets;
mod content;
mod go;
mod rust;
mod schema;

use std::{env, fs, path::Path};

use anyhow::{Context, Result, ensure};
pub use schema::{Definition, FieldType, Message, Project, valid_name};

/// Generates the Go module manifest for the pinned native server-helper contract.
pub fn go_module(id: &str) -> String {
    go::module(id)
}

/// Generates this Cargo project's shared module and metadata in Cargo's output directory.
/// Call from `build.rs`; the source is `src/shared.rs` and package files live in `assets/`.
pub fn generate() -> Result<()> {
    let root = env::var_os("CARGO_MANIFEST_DIR").context("CARGO_MANIFEST_DIR missing")?;
    let out = env::var_os("OUT_DIR").context("OUT_DIR missing")?;
    generate_at(Path::new(&root), Path::new(&out))?;
    Ok(())
}

/// Generates deterministic bindings and returns the parsed project for packaging tools.
pub fn generate_at(root: &Path, out: &Path) -> Result<Project> {
    let source = root.join("src/shared.rs");
    println!("cargo:rerun-if-changed={}", source.display());
    let project = Project::parse(
        &fs::read_to_string(&source).with_context(|| format!("reading {}", source.display()))?,
    )?;
    let assets = assets::scan(&root.join("assets"))?;
    for definition in &project.definitions {
        if let Some(content) = &definition.content {
            let name = content
                .asset
                .path
                .segments
                .last()
                .expect("checked asset path")
                .ident
                .to_string();
            ensure!(
                assets.iter().any(|(path, _)| assets::constant(path) == name
                    && path.ends_with(".png")
                    && !path.starts_with("server/")
                    && !path.starts_with("client/")),
                "{} needs a shared PNG asset constant",
                definition.name
            );
        }
    }
    let mut rust = rust::generate(&project)?;
    rust.push_str(&assets::generate(&assets)?);
    rust.push_str(&content::generate(&project));
    fs::create_dir_all(out)?;
    fs::write(out.join("mod_shared.rs"), rust)?;
    fs::write(out.join("messages.go"), go::generate(&project)?)?;
    fs::write(
        out.join("registry.json"),
        serde_json::to_vec_pretty(&project.document())?,
    )?;
    Ok(project)
}

/// Returns the manifest API from the canonical server WIT package version.
pub fn server_api() -> &'static str {
    include_str!("../wit/server/server.wit")
        .lines()
        .find_map(|line| line.strip_prefix("package ")?.strip_suffix(';'))
        .and_then(|package| package.rsplit_once('@'))
        .and_then(|(_, version)| version.rsplit_once('.'))
        .map(|(api, _)| api)
        .expect("server WIT has a versioned package")
}

#[cfg(test)]
mod tests;
