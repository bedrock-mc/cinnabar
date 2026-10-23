//! Builds shared Rust definitions, channels, Go codecs and asset constants from one source.
#![expect(
    clippy::print_stdout,
    reason = "build scripts report Cargo dependency tracking directives"
)]

mod assets;
mod go;
mod rust;
mod schema;

use std::{env, fs, path::Path};

use anyhow::{Context, Result};
pub use schema::{Definition, FieldType, Message, Project, valid_name};

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
    let mut rust = rust::generate(&project)?;
    rust.push_str(&assets::generate(&assets)?);
    fs::create_dir_all(out)?;
    fs::write(out.join("mod_shared.rs"), rust)?;
    fs::write(out.join("messages.go"), go::generate(&project)?)?;
    fs::write(
        out.join("registry.json"),
        serde_json::to_vec_pretty(&project.document())?,
    )?;
    Ok(project)
}

#[cfg(test)]
mod tests;
