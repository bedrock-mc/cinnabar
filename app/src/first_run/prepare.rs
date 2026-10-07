//! One preparation run: fetch the pack only when a stale carrier reads it, rebuild the stale
//! carriers with a single `assetc prepare` and publish the set over the previous one.

use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::Path,
    sync::atomic::AtomicBool,
};

use anyhow::{Context, Result, bail};
use assets::carriers::{self, COMPILED_DIR, FONT_MANIFEST, Sources};
use serde::Deserialize;

use super::{
    download,
    runner::{self, Compiler, Event, Selection},
    status::{Phase, Status},
};
use crate::install_layout::InstallLayout;

const UNPACK_LABEL: &str = "Unpacking the Minecraft sample resource pack";

/// The bundled UI font's file name, from the kit's copy of the font manifest.
pub(super) fn ui_font_file(kit: &Path) -> Result<String> {
    #[derive(Deserialize)]
    struct FontSource {
        font_file: String,
    }
    let path = Sources::Kit(kit.to_path_buf()).resolve(FONT_MANIFEST);
    let bytes = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
    let font: FontSource =
        serde_json::from_slice(&bytes).with_context(|| format!("parse {}", path.display()))?;
    Ok(font.font_file)
}

fn compiler<'a>(
    layout: &InstallLayout,
    log: Option<File>,
    cancel: &'a AtomicBool,
) -> Result<Compiler<'a>> {
    let kit = layout.prep_kit();
    if !kit.is_dir() {
        bail!(
            "installer preparation kit is missing at {}; reinstall Cinnabar",
            kit.display()
        );
    }
    Ok(Compiler {
        kit,
        workspace: layout.prepare_workspace(),
        log,
        cancel,
    })
}

/// The published carriers the kit's pins make stale.
pub(super) fn selection(layout: &InstallLayout) -> Result<Selection> {
    let prepared = layout.prepared_assets_dir();
    runner::recover(&prepared);
    compiler(layout, None, &AtomicBool::new(false))?.check(&prepared)
}

/// Consent-free preparation. Ends with a `Done` or `Failed` report; failures also go to the log.
pub(super) fn prepare(
    layout: &InstallLayout,
    cancel: &AtomicBool,
    report: &mut dyn FnMut(Status),
) -> Result<()> {
    let result = open_log(layout).and_then(|log| run(layout, cancel, report, log));
    match &result {
        Ok(()) => report(Status::new(Phase::Done, 0, 0, "Ready")),
        Err(error) => {
            let message = format!("{error:#}");
            if let Ok(mut log) = open_log(layout) {
                let _ = writeln!(log, "first-run preparation failed: {message}");
            }
            report(Status::failed("Failed", &message));
        }
    }
    result
}

fn open_log(layout: &InstallLayout) -> Result<File> {
    fs::create_dir_all(layout.log_dir())?;
    let path = layout.log_dir().join("first-run.log");
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("open {}", path.display()))
}

fn run(
    layout: &InstallLayout,
    cancel: &AtomicBool,
    report: &mut dyn FnMut(Status),
    log: File,
) -> Result<()> {
    let prepared = layout.prepared_assets_dir();
    runner::recover(&prepared);
    let compiler = compiler(layout, Some(log), cancel)?;
    let selection = compiler.check(&prepared)?;
    let (kit, workspace) = (&compiler.kit, &compiler.workspace);
    let unpack = usize::from(selection.needs_pack);
    let total = unpack + selection.stale.len();
    if selection.needs_pack {
        download::fetch_archive(kit, workspace, cancel, |received, total| {
            report(Status::downloading(received, total));
        })?;
        report(Status::new(Phase::Running, 1, total, UNPACK_LABEL));
        download::unpack(kit, workspace, cancel).context(UNPACK_LABEL)?;
    }
    let staged = workspace.join(COMPILED_DIR);
    if staged.exists() {
        fs::remove_dir_all(&staged)?;
    }
    fs::create_dir_all(&staged)?;
    // Current carriers and their stamp carry over; prepare replaces the stale ones.
    runner::seed(&prepared, &staged)?;
    let mut finished = 0;
    let mut running: Vec<(String, String)> = Vec::new();
    compiler.prepare(&staged, |event| {
        match event {
            Event::Start { name, label } => running.push((name.clone(), label.clone())),
            Event::Done { name } | Event::Failed { name, .. } => {
                running.retain(|(running, _)| running != name);
                finished += 1;
            }
            Event::Plan { .. } => return,
        }
        // The longest-running carrier names the step.
        if let Some((_, label)) = running.first() {
            let step = (unpack + finished + 1).min(total);
            report(Status::new(Phase::Running, step, total, label));
        }
    })?;
    if !carriers::required_present(&staged) {
        bail!(
            "preparation finished but required carriers are missing under {}",
            staged.display()
        );
    }
    runner::publish_if_active(&staged, &prepared, cancel)?;
    // Only the current pack's archive is kept, so a carrier-only update needs no download.
    let _ = fs::remove_dir_all(workspace.join(".local/assets/bedrock-samples"));
    download::prune(kit, workspace);
    Ok(())
}
