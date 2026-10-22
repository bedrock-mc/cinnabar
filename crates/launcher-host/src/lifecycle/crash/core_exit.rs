//! `kind: core` folders for a bedrock-core crash, written by the client, which outlives it.

use std::path::PathBuf;

use crash_report::{
    folder::{self, Attachment},
    report::{Kind, Thread},
};

use super::{Context, attachments::Attachments};
use crate::lifecycle::core_output::CoreCrash;

/// Writes the folder for `crash` with this session's state, log sections and staged
/// attachments, plus the Go crash output as [`folder::CORE_CRASH_FILE`]; `None` before crash
/// capture is installed or when the folder could not be written, which is logged.
pub(crate) fn report_core(crash: &CoreCrash) -> Option<PathBuf> {
    let context = super::CONTEXT.get()?;
    write(context, crash, super::now_ms())
        .inspect_err(|error| tracing::warn!("write the core crash report: {error}"))
        .ok()
}

/// [`report_core`] into `context` at `now_ms`.
pub(super) fn write(context: &Context, crash: &CoreCrash, now_ms: u64) -> std::io::Result<PathBuf> {
    let mut report = super::report(
        context,
        Kind::Core,
        crash.message(),
        &super::snapshot(context),
        now_ms,
    );
    report.thread = Thread {
        name: Some(format!("bedrock-core {}", crash.role.label())),
        id: crash.pid.to_string(),
    };
    let logs = folder::Logs::read(&context.log_sources());
    // The packet and flight rings show what the client saw around the core's crash.
    let staged = context
        .staging
        .as_ref()
        .map(|staging| Attachments::read(staging.dir()))
        .unwrap_or_default();
    let mut attachments = staged.as_folder();
    if !crash.crash_output.is_empty() {
        attachments.push(Attachment {
            name: folder::CORE_CRASH_FILE,
            bytes: &crash.crash_output,
        });
    }
    let folder = folder::write_with(
        &context.crash_dir,
        &report,
        &logs,
        None,
        &attachments,
        now_ms,
    )?;
    super::upload::completed_with_settings(&folder, &context.settings_file, now_ms);
    Ok(folder)
}

#[cfg(test)]
#[path = "core_exit_tests.rs"]
mod tests;
