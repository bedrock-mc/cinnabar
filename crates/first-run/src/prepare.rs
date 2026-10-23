//! One preparation run: fetch the pack only when a stale carrier reads it, rebuild the stale
//! carriers with a single `assetc prepare` and publish the set over the previous one.

use std::{fs, io::Write, path::PathBuf, sync::atomic::AtomicBool};

use anyhow::{Context, Result, bail};
use assets::carriers::{self, COMPILED_DIR};
use diagnostics::session_log::{Family, Retention, SessionLog, SharedLog};

use super::{
    download,
    runner::{self, Compiler, Event, Selection},
    status::{Phase, Status},
};
use launcher::install_layout::InstallLayout;

pub(super) const UNPACK_LABEL: &str = "Unpacking the Minecraft sample resource pack";

fn compiler<'a>(
    layout: &InstallLayout,
    log: Option<SharedLog>,
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
        #[cfg(unix)]
        own_group: !launcher_host::lifecycle::update::launched_by_updater(),
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
    let result = prepare_assets(layout, cancel, report);
    match &result {
        Ok(()) => report(Status::ready()),
        Err(error) => report(Status::failed("Failed", &format!("{error:#}"))),
    }
    result
}

/// Prepares and publishes carriers without reporting overall setup completion.
pub(super) fn prepare_assets(
    layout: &InstallLayout,
    cancel: &AtomicBool,
    report: &mut dyn FnMut(Status),
) -> Result<()> {
    let mut session_log = None;
    let result = open_log(layout).and_then(|log| {
        session_log = Some(log.clone());
        run(layout, cancel, report, log)
    });
    if let Err(error) = &result
        && let Some(log) = &mut session_log
    {
        let _ = writeln!(log, "first-run preparation failed: {error:#}");
    }
    result
}

/// The setup log and its retained earlier sessions.
fn log_family(layout: &InstallLayout) -> Family {
    Family::new(layout.log_dir(), "first-run", "log")
}

/// The current setup log, shared by asset and local-world preparation.
pub(super) fn log_path(layout: &InstallLayout) -> PathBuf {
    log_family(layout).current()
}

/// Starts this run's `first-run.log`, archiving earlier runs under the client log's retention.
fn open_log(layout: &InstallLayout) -> Result<SharedLog> {
    let family = log_family(layout);
    let path = family.current();
    let log = SessionLog::open(family, Retention::DEFAULT, Vec::new())
        .with_context(|| format!("open {}", path.display()))?;
    Ok(SharedLog::new(log))
}

fn run(
    layout: &InstallLayout,
    cancel: &AtomicBool,
    report: &mut dyn FnMut(Status),
    log: SharedLog,
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
