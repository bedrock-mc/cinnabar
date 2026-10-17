//! Translates every program of a shader pack and prints a pass/fail summary.
//!
//! `cargo run -p shaderpack --example translate -- <pack folder or zip> [--folder world0]
//! [--enabled-only] [--no-dh] [--verbose]`
//!
//! By default programs the pack disables are translated too (their enable rule's options are
//! switched on) and Distant Horizons programs are included, so every stage file is covered.

use shaderpack::{ShaderPack, TranslateConfig};
use std::path::PathBuf;
use std::process::ExitCode;

#[expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "command-line report"
)]
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |f: &str| args.iter().any(|a| a == f);
    let value = |f: &str| {
        args.iter()
            .position(|a| a == f)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let Some(path) = args
        .iter()
        .find(|a| !a.starts_with("--") && Some(*a) != value("--folder").as_ref())
    else {
        eprintln!(
            "usage: translate <pack folder or zip> [--folder world0] [--enabled-only] [--no-dh] [--verbose]"
        );
        return ExitCode::FAILURE;
    };
    let pack = match ShaderPack::open(&PathBuf::from(path)) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let mut config = TranslateConfig {
        force_disabled: !flag("--enabled-only"),
        ..TranslateConfig::default()
    };
    config.environment.distant_horizons = !flag("--no-dh");
    if let Some(folder) = value("--folder") {
        config.folder = folder;
    }
    let verbose = flag("--verbose");
    let started = std::time::Instant::now();
    let result = pack.translate(&config);
    let elapsed = started.elapsed();

    let (mut files, mut files_ok, mut programs, mut programs_ok, mut skipped) = (0, 0, 0, 0, 0);
    for p in &result.programs {
        let name = format!(
            "{}{}",
            p.program.name,
            if p.program.kind == shaderpack::ProgramKind::Compute {
                " (compute)"
            } else {
                ""
            }
        );
        if p.stages.is_empty() && !p.enabled {
            skipped += 1;
            println!("SKIP {name}: disabled");
            continue;
        }
        programs += 1;
        files += p.stages.len();
        files_ok += p.stages.iter().filter(|s| s.result.is_ok()).count();
        let state = if !p.enabled { " [forced]" } else { "" };
        if p.is_ok() {
            programs_ok += 1;
            println!("PASS {name}{state}");
            if verbose && let Some(r) = &p.reflection {
                println!(
                    "     inputs {}, uniforms {}, textures {}, images {}, outputs {:?}",
                    r.vertex_inputs.len(),
                    r.uniforms.as_ref().map_or(0, |u| u.members.len()),
                    r.textures.len(),
                    r.storage_images.len(),
                    r.color_outputs
                        .iter()
                        .map(|o| (o.location, o.buffer, o.format.name()))
                        .collect::<Vec<_>>()
                );
            }
        } else {
            println!("FAIL {name}{state}");
            for s in &p.stages {
                if let Err(diags) = &s.result {
                    for d in diags.iter().take(if verbose { usize::MAX } else { 1 }) {
                        println!("     {}: {d}", s.path);
                    }
                }
            }
            for d in &p.diagnostics {
                println!("     {d}");
            }
        }
    }
    let options = pack.options().len();
    println!();
    println!("stage files: {files_ok}/{files} validated");
    println!("programs:    {programs_ok}/{programs} translated, {skipped} disabled and skipped");
    println!(
        "options:     {options}; buffers with directives: {}",
        result.buffers.buffers.len()
    );
    println!("time:        {:.2}s", elapsed.as_secs_f64());
    ExitCode::SUCCESS
}
