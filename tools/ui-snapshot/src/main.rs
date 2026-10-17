//! Window-free, service-free screenshots through the production UI painters.
#![expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "ui-snapshot prints listings and image metadata on stdout, and failures and fixture skips on stderr"
)]

mod carriers;
mod fixtures;
mod render;
#[cfg(test)]
mod tests;

use std::path::PathBuf;

/// Parses the small CLI and reports missing local fixtures without emitting a fake image.
fn main() {
    init_logging();
    if let Err(message) = run() {
        eprintln!("{message}");
        std::process::exit(1);
    }
}

/// Lists fixtures or writes a PNG and its viewport/backend metadata beside it.
fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let mut name = None;
    let mut out = None;
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.local/assets/compiled");
    let mut list = false;
    let mut seconds = render::CAPTURE_SECONDS;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--list" => list = true,
            "--state" => name = Some(args.next().ok_or("--state requires a name")?),
            "--out" => {
                out = Some(PathBuf::from(
                    args.next().ok_or("--out requires a PNG path")?,
                ))
            }
            "--carriers" => {
                dir = PathBuf::from(args.next().ok_or("--carriers requires a directory")?)
            }
            "--time" => {
                seconds = args
                    .next()
                    .ok_or("--time requires seconds")?
                    .parse::<f64>()
                    .map_err(|_| "--time requires finite, nonnegative seconds")?;
                if !seconds.is_finite() || seconds < 0.0 {
                    return Err("--time requires finite, nonnegative seconds".into());
                }
            }
            "--help" | "-h" => {
                println!(
                    "ui-snapshot --list | --state NAME --out PNG [--carriers DIR] [--time SECONDS]"
                );
                return Ok(());
            }
            _ => return Err(format!("unknown argument {arg}; use --help")),
        }
    }
    if list {
        for state in fixtures::STATES {
            println!(
                "{} {}x{} gui-scale={}",
                state.name, state.size[0], state.size[1], state.gui_scale
            );
        }
        return Ok(());
    }
    let name = name.ok_or("--state is required; use --list")?;
    let state = fixtures::STATES
        .iter()
        .find(|state| state.name == name)
        .ok_or_else(|| format!("unknown state {name}; use --list"))?;
    let out = out.ok_or("--out is required")?;
    if out.extension().is_none_or(|extension| extension != "png") {
        return Err("--out must have a .png extension".into());
    }
    let png = match render::png_at(state, &dir, seconds) {
        Ok(png) => png,
        Err(carriers::Error::Missing(message)) => {
            eprintln!("skipping {}: missing fixture {message}", state.name);
            return Ok(());
        }
        Err(carriers::Error::Invalid(message)) => return Err(message),
    };
    let metadata = serde_json::json!({
        "state": state.name, "size": state.size, "gui_scale": state.gui_scale,
        "dpi": 1, "backend": "cpu", "time_seconds": seconds,
    });
    std::fs::write(&out, png).map_err(|error| format!("{}: {error}", out.display()))?;
    std::fs::write(out.with_extension("json"), format!("{metadata}\n"))
        .map_err(|error| error.to_string())?;
    println!("{metadata}");
    Ok(())
}

/// Installs stderr diagnostics before any artwork loader can fall back.
fn init_logging() {
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .try_init();
}
