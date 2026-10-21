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
    let mut panel = None;
    let mut resource_pack = None;
    let mut size = [1280u32, 720u32];
    let mut gui_scale = 2u8;
    let mut seconds = render::CAPTURE_SECONDS;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--list" => list = true,
            "--resource-pack" => {
                resource_pack = Some(PathBuf::from(
                    args.next().ok_or("--resource-pack requires a directory")?,
                ))
            }
            "--panel" => {
                panel = Some(PathBuf::from(
                    args.next().ok_or("--panel requires a JSON path")?,
                ))
            }
            "--width" => {
                size[0] = args
                    .next()
                    .ok_or("--width requires pixels")?
                    .parse::<u32>()
                    .map_err(|error| error.to_string())?
            }
            "--height" => {
                size[1] = args
                    .next()
                    .ok_or("--height requires pixels")?
                    .parse::<u32>()
                    .map_err(|error| error.to_string())?
            }
            "--gui-scale" => {
                gui_scale = args
                    .next()
                    .ok_or("--gui-scale requires a scale")?
                    .parse::<u8>()
                    .map_err(|error| error.to_string())?
            }
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
                    "ui-snapshot --list | (--state NAME | --panel JSON) --out PNG [--carriers DIR] [--resource-pack DIR] [--width PX --height PX --gui-scale SCALE --time SECONDS]"
                );
                return Ok(());
            }
            _ => return Err(format!("unknown argument {arg}; use --help")),
        }
    }
    if list {
        for state in fixtures::states() {
            println!(
                "{} {}x{} gui-scale={}",
                state.name, state.size[0], state.size[1], state.gui_scale
            );
        }
        return Ok(());
    }
    if panel.is_some() && name.is_some() {
        return Err("--panel and --state are mutually exclusive".into());
    }
    if size.iter().any(|value| !(1..=8192).contains(value))
        || !(1..=4).contains(&gui_scale)
        || !seconds.is_finite()
        || seconds < 0.0
        || (panel.is_some() && seconds > 60.0)
    {
        return Err("snapshot dimensions, GUI scale or sample time exceed their bounds".into());
    }
    let state = match name.as_ref() {
        Some(name) => Some(
            fixtures::states()
                .find(|state| state.name == name)
                .ok_or_else(|| format!("unknown state {name}; use --list"))?,
        ),
        None if panel.is_some() => None,
        None => return Err("--state or --panel is required; use --list".into()),
    };
    let out = out.ok_or("--out is required")?;
    if out.extension().is_none_or(|extension| extension != "png") {
        return Err("--out must have a .png extension".into());
    }
    let rendered = if let Some(path) = &panel {
        let bytes = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
        if bytes.len() > ui::mod_panel::MAX_PANEL_BUNDLE_BYTES {
            return Err("panel JSON exceeds its byte limit".into());
        }
        let panel = serde_json::from_slice(&bytes)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        render::panel_png(
            panel,
            size,
            gui_scale,
            seconds,
            &dir,
            resource_pack.as_deref(),
        )
    } else {
        render::png_at(state.expect("named state checked"), &dir, seconds)
    };
    let (name, size, gui_scale) = state.map_or(("mod-panel", size, gui_scale), |state| {
        (state.name, state.size, state.gui_scale)
    });
    let png = match rendered {
        Ok(png) => png,
        Err(carriers::Error::Missing(message)) => {
            eprintln!("skipping {name}: missing fixture {message}");
            return Ok(());
        }
        Err(carriers::Error::Invalid(message)) => return Err(message),
    };
    let metadata = serde_json::json!({
        "state": name, "size": size, "gui_scale": gui_scale,
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
