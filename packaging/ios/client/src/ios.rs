//! The bundle supplies the read-only resources and the app container the writable roots; the Go
//! core is linked into this executable and runs in-process.

use std::{fs, io::Write, path::PathBuf};

use anyhow::{Context, Result};
use launcher::install_layout::{InstallLayout, configure_ios};

/// Names shared with `packaging/ios/build-ipa.sh` and `add-assets.py`.
#[derive(serde::Deserialize)]
struct Runtime {
    /// The bundle directory mirroring `.local/`. iOS reads a top-level `Resources` or
    /// `Contents` directory, in any case, as bundle structure and then rejects the app.
    resource_dir: String,
}

unsafe extern "C" {
    /// Returns once the linked Go runtime has initialized (`core/cmd/bedrock-core/embed_ios.go`).
    fn CinnabarCoreInit();
}

pub(crate) fn main() {
    let code = match run_client() {
        Ok(()) => 0,
        Err(error) => {
            // The console mirrors into the session's client.log, the one place an iOS user can
            // read why the app closed; the process's own stderr goes nowhere.
            let _ = writeln!(
                diagnostics::console::stderr(),
                "{} failed: {error:#}",
                launcher::PRODUCT_NAME
            );
            diagnostics::console::flush_before_exit();
            1
        }
    };
    // UIKit's run loop never returns once the window opens, so this ends only failed starts.
    std::process::exit(code);
}

fn run_client() -> Result<()> {
    let runtime: Runtime = serde_json::from_str(include_str!("../../runtime.json"))
        .context("parse the tracked iOS runtime manifest")?;
    let executable = std::env::current_exe().context("iOS reported no app executable")?;
    let bundle = executable
        .parent()
        .context("the app executable is outside its bundle")?;
    let container =
        PathBuf::from(std::env::var_os("HOME").context("iOS reported no app container directory")?);
    let layout = InstallLayout::ios(
        bundle.join(&runtime.resource_dir),
        &container,
        executable.clone(),
    );
    // Bridge endpoints whose socket path would not fit sun_path are relative to the working
    // directory, which must be this private runtime root before any core starts. Go's runtime
    // moves the working directory to the bundle while it initializes on a thread of its own, so
    // wait for it first; otherwise the cores race that move and fail to bind in the bundle.
    // SAFETY: the export takes no arguments and only waits for runtime initialization.
    unsafe { CinnabarCoreInit() };
    let runtime_root = layout.transient_runtime_root();
    fs::create_dir_all(runtime_root)
        .with_context(|| format!("create {}", runtime_root.display()))?;
    std::env::set_current_dir(runtime_root)
        .with_context(|| format!("enter {}", runtime_root.display()))?;
    configure_ios(layout)?;
    if cinnabar::lifecycle::before_run(false)? {
        cinnabar::run(cinnabar::args::ClientArgs {
            force_vsync: true,
            ..Default::default()
        })?;
    }
    Ok(())
}
