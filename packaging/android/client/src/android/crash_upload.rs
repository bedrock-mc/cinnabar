//! Android sends only from the healthy next launch, using the desktop payload and sender.

use jni::{
    JNIEnv,
    objects::{JByteArray, JObject, JString},
    sys::{jboolean, jint, jlong, jstring},
};
use launcher::install_layout::InstallLayout;
use launcher_host::lifecycle::crash::upload::{
    self,
    previous_exit::{self, Exit, Snapshot},
};
use std::{path::Path, sync::OnceLock};

/// Frozen once before BootstrapActivity exposes Retry or starts a new game session.
static PREVIOUS: OnceLock<Snapshot> = OnceLock::new();

/// Captures previous-session bytes on the healthy bootstrap worker, without any networking.
fn previous(layout: &InstallLayout) -> &'static Snapshot {
    PREVIOUS.get_or_init(|| {
        Snapshot::capture(&layout.crash_dir(), &layout.log_dir().join("client.log"))
    })
}

/// Retains the previous log before startup and returns the saved report's opaque local identity.
pub(super) extern "system" fn capture<'local>(
    mut env: JNIEnv<'local>,
    _activity: JObject<'local>,
    files: JString<'local>,
    libraries: JString<'local>,
) -> jstring {
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
        || -> anyhow::Result<Option<String>> {
            let files = String::from(env.get_string(&files)?);
            let libraries = String::from(env.get_string(&libraries)?);
            let layout = super::layout(Path::new(&files), Path::new(&libraries));
            Ok(previous(&layout)
                .latest_saved()
                .and_then(Path::file_name)
                .map(|name| name.to_string_lossy().into_owned()))
        },
    ));
    reply(&mut env, outcome.ok().and_then(Result::ok).flatten())
}

/// Records Android's previous exit, optionally consents to it, and retries due reports.
/// Timestamp zero completes the frozen prior session's saved reports on every supported API.
#[allow(clippy::too_many_arguments, reason = "JNI fixes the native method ABI")]
pub(super) extern "system" fn report<'local>(
    mut env: JNIEnv<'local>,
    _activity: JObject<'local>,
    files: JString<'local>,
    libraries: JString<'local>,
    timestamp: jlong,
    reason: jint,
    status: jint,
    description: JString<'local>,
    trace: JByteArray<'local>,
    user: jboolean,
) -> jstring {
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
        || -> anyhow::Result<String> {
            let files = String::from(env.get_string(&files)?);
            let libraries = String::from(env.get_string(&libraries)?);
            let layout = super::layout(Path::new(&files), Path::new(&libraries));
            let Some(policy) = upload::Policy::load(&layout) else {
                return Ok("Crash uploads are disabled".into());
            };
            let snapshot = previous(&layout);
            let target = if timestamp > 0 {
                let description = String::from(env.get_string(&description)?);
                let trace = if trace.is_null() {
                    Vec::new()
                } else {
                    env.convert_byte_array(&trace)?
                };
                let exit = Exit {
                    timestamp_ms: timestamp as u64,
                    reason,
                    status,
                    description: &description,
                    tombstone: (!trace.is_empty()).then_some(trace.as_slice()),
                };
                Some(previous_exit::record(
                    &layout.crash_dir(),
                    snapshot,
                    &exit,
                    |name| name == format!("lib{}.so", env!("CARGO_CRATE_NAME")),
                )?)
            } else {
                for folder in snapshot.complete_saved()? {
                    upload::completed(&layout, &folder);
                }
                snapshot.latest_saved().map(Path::to_owned)
            };
            if let Some(folder) = &target {
                if user != 0 {
                    upload::consent(
                        folder,
                        crash_report::upload::state::Consent::User,
                        upload::now_ms(),
                    )?;
                }
                upload::completed(&layout, folder);
            }
            for (folder, result) in upload::send_due(&layout, &policy) {
                upload::log_result(&folder, &result);
            }
            Ok(target
                .as_deref()
                .map(report_status)
                .unwrap_or_default()
                .to_owned())
        },
    ));
    let message = match outcome {
        Ok(Ok(message)) => message,
        _ => "Could not prepare crash report".into(),
    };
    reply(&mut env, Some(message))
}

/// Describes persisted delivery state without exposing report paths or identifiers.
fn report_status(folder: &Path) -> &'static str {
    use crash_report::upload::state::{self, Upload};
    match state::read(folder) {
        Some(Upload::Sent { .. }) => "Report sent",
        Some(Upload::Pending { .. }) => "Report queued; will retry on a later launch",
        Some(Upload::Abandoned { .. }) => "Report could not be sent",
        None => "Report saved on this device",
    }
}

/// Clears pending Java exceptions and creates the nullable native return string.
fn reply(env: &mut JNIEnv<'_>, message: Option<String>) -> jstring {
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }
    message
        .and_then(|message| env.new_string(message).ok())
        .map_or(std::ptr::null_mut(), JString::into_raw)
}
