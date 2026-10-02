//! One preparation run: rebuild the stale carriers (downloading the pack only when a stale one
//! reads it), stamp the set and publish it over the previous one.

use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    sync::atomic::AtomicBool,
};

use anyhow::{Context, Result, bail};

use super::{
    download, plan, runner,
    stamp::{self, Selection},
    status::{Phase, Status},
};
use crate::install_layout::InstallLayout;

/// The current plan against the published carriers.
pub(super) fn selection(layout: &InstallLayout) -> Result<(Vec<plan::Step>, Selection)> {
    let kit = layout.prep_kit();
    if !kit.is_dir() {
        bail!(
            "installer preparation kit is missing at {}; reinstall Cinnabar",
            kit.display()
        );
    }
    let prepared = layout.prepared_assets_dir();
    runner::recover(&prepared);
    let steps = plan::steps(&kit)?;
    let selection = stamp::select(&steps, &kit, &prepared)?;
    Ok((steps, selection))
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
    let (steps, selection) = selection(layout)?;
    let (kit, workspace, prepared) = (
        layout.prep_kit(),
        layout.prepare_workspace(),
        layout.prepared_assets_dir(),
    );
    runner::stage_kit(&kit, &workspace)?;
    if selection.needs_pack {
        download::fetch_archive(&workspace, cancel, |received, total| {
            report(Status::downloading(received, total));
        })?;
    }
    let staged = workspace.join(plan::COMPILED);
    if staged.exists() {
        fs::remove_dir_all(&staged)?;
    }
    fs::create_dir_all(&staged)?;
    // Current carriers carry over; the steps that rerun replace theirs.
    if plan::carriers_present(&prepared) {
        runner::seed(&prepared, &staged)?;
    }
    for output in &selection.outputs {
        runner::clear_output(&staged, output);
    }
    let stale: Vec<plan::Step> = steps
        .into_iter()
        .zip(&selection.run)
        .filter_map(|(step, run)| run.then_some(step))
        .collect();
    let exec = runner::ProcessExec {
        workspace: workspace.clone(),
        kit,
        log,
        cancel,
    };
    let total = stale.len();
    runner::execute_steps(
        &stale,
        |step| exec.run(step),
        |index, step| report(Status::new(Phase::Running, index + 1, total, step.label)),
    )?;
    if !plan::carriers_present(&staged) {
        bail!(
            "preparation finished but required carriers are missing under {}",
            staged.display()
        );
    }
    stamp::write(&staged, &selection.identities)?;
    runner::publish_if_active(&staged, &prepared, cancel)?;
    // Only the current pack's archive is kept, so a carrier-only update needs no download.
    let _ = fs::remove_dir_all(workspace.join(".local/assets/bedrock-samples"));
    download::prune(&workspace);
    Ok(())
}
