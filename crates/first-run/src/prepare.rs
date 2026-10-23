//! One preparation run: fetch the pack only when a stale carrier reads it, rebuild the stale
//! carriers with a single `assetc prepare` and publish the set over the previous one.

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

use anyhow::{Context, Result, bail};
use assets::carriers::{self, COMPILED_DIR};
use diagnostics::session_log::{Family, Retention, SessionLog, SharedLog};

use super::{
    download,
    runner::{self, Compiler, Event, Selection},
    status::{Phase, Status},
};
use launcher::install_layout::{InstallLayout, vanilla_pack_relative};

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
    // Server-pack entities may name vanilla textures, so an install keeps them beside its carriers.
    let needs_pack = selection.needs_pack || !vanilla_textures_kept(layout);
    let unpack = usize::from(needs_pack);
    let total = unpack + selection.stale.len();
    if needs_pack {
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
    if needs_pack {
        keep_vanilla_textures(
            &workspace.join(".local").join(vanilla_pack_relative()),
            &layout.vanilla_pack_dir(),
        )?;
    }
    // Only the current pack's archive is kept, so a carrier-only update needs no download.
    let _ = fs::remove_dir_all(workspace.join(".local/assets/bedrock-samples"));
    download::prune(kit, workspace);
    Ok(())
}

/// Directory under the kept vanilla pack that holds its texture images.
const TEXTURES_DIR: &str = "textures";

/// True when this install already kept the current pin's vanilla texture images.
pub(super) fn vanilla_textures_kept(layout: &InstallLayout) -> bool {
    layout.vanilla_pack_dir().join(TEXTURES_DIR).is_dir()
}

/// Copies the unpacked pack's `.png` and `.tga` textures to `kept`, replacing what it held.
fn keep_vanilla_textures(unpacked: &Path, kept: &Path) -> Result<()> {
    let source = unpacked.join(TEXTURES_DIR);
    let staged = kept.with_extension("partial");
    let _ = fs::remove_dir_all(&staged);
    copy_images(&source, &staged.join(TEXTURES_DIR))
        .with_context(|| format!("keep vanilla textures from {}", source.display()))?;
    if kept.exists() {
        fs::remove_dir_all(kept).with_context(|| format!("replace {}", kept.display()))?;
    }
    fs::rename(&staged, kept).with_context(|| format!("publish {}", kept.display()))?;
    Ok(())
}

/// Copies every `.png` and `.tga` file below `from` to the same relative path below `to`.
fn copy_images(from: &Path, to: &Path) -> Result<()> {
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let path = entry.path();
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_images(&path, &target)?;
        } else if path
            .extension()
            .is_some_and(|extension| extension == "png" || extension == "tga")
        {
            fs::create_dir_all(to)?;
            fs::copy(&path, &target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod kept_texture_tests {
    use super::*;
    use crate::test_support::Dir;

    #[test]
    fn keeps_only_texture_images_and_replaces_an_earlier_copy() {
        let dir = Dir::new("keep-vanilla-textures");
        let unpacked = dir.path().join("unpacked");
        let blocks = unpacked.join(TEXTURES_DIR).join("blocks");
        fs::create_dir_all(&blocks).unwrap();
        fs::write(blocks.join("wool_colored_white.png"), b"png").unwrap();
        fs::write(blocks.join("basalt_side_mers.tga"), b"tga").unwrap();
        fs::write(blocks.join("wool_colored_white.texture_set.json"), b"{}").unwrap();
        let kept = dir.path().join("kept/resource_pack");
        fs::create_dir_all(kept.join(TEXTURES_DIR)).unwrap();
        fs::write(kept.join(TEXTURES_DIR).join("stale.png"), b"old").unwrap();

        keep_vanilla_textures(&unpacked, &kept).unwrap();

        let kept_blocks = kept.join(TEXTURES_DIR).join("blocks");
        assert_eq!(
            fs::read(kept_blocks.join("wool_colored_white.png")).unwrap(),
            b"png"
        );
        assert!(kept_blocks.join("basalt_side_mers.tga").is_file());
        assert!(
            !kept_blocks
                .join("wool_colored_white.texture_set.json")
                .exists()
        );
        assert!(!kept.join(TEXTURES_DIR).join("stale.png").exists());
        assert!(!kept.with_extension("partial").exists());
    }
}
