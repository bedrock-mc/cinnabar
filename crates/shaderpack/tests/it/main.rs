#![expect(
    clippy::print_stderr,
    reason = "fixture helpers explain on stderr why a test skips when a fixture pack is missing"
)]

//! Whole-pack translation of the small original packs in `tests/fixtures`.

mod compiler_contracts;
mod compute;
mod dimensions;
mod draw_buffers;
mod includes;
mod legacy;
mod options;
mod samplers;
mod zip_source;

use shaderpack::{PackTranslation, ProgramReflection, ProgramTranslation, ShaderPack};
use std::path::PathBuf;

/// The folder of fixture pack `name` (it holds `shaders/`).
fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

/// Opens fixture pack `name`, or names the missing fixture and returns `None` so the caller
/// skips.
fn open_fixture(name: &str) -> Option<ShaderPack> {
    let path = fixture_path(name);
    if !path.join("shaders").is_dir() {
        eprintln!("missing fixture: shader pack {}; skipping", path.display());
        return None;
    }
    Some(
        ShaderPack::open(&path)
            .unwrap_or_else(|e| panic!("open fixture pack {}: {e}", path.display())),
    )
}

/// The translation of program `name` (the first one, render before compute).
fn program<'a>(t: &'a PackTranslation, name: &str) -> &'a ProgramTranslation {
    t.programs
        .iter()
        .find(|p| p.program.name == name)
        .unwrap_or_else(|| {
            let names: Vec<&str> = t.programs.iter().map(|p| p.program.name.as_str()).collect();
            panic!("no program `{name}` in {names:?}")
        })
}

/// The reflection of a program that must translate, failing with its diagnostics otherwise.
fn reflection(p: &ProgramTranslation) -> &ProgramReflection {
    p.reflection.as_ref().unwrap_or_else(|| {
        let diagnostics: Vec<String> = p.all_diagnostics().map(ToString::to_string).collect();
        panic!(
            "program `{}` did not translate: {diagnostics:#?}",
            p.program.name
        )
    })
}

/// Names of a program's sampled textures, in binding order.
fn texture_names(r: &ProgramReflection) -> Vec<&str> {
    r.textures.iter().map(|t| t.name.as_str()).collect()
}

/// Names of the loose-uniform block members, in offset order (empty without a block).
fn uniform_names(r: &ProgramReflection) -> Vec<&str> {
    r.uniforms
        .iter()
        .flat_map(|u| &u.members)
        .map(|m| m.name.as_str())
        .collect()
}
