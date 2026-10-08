//! Tool arguments mapped onto control commands, with paths resolved against the checkout.

use std::{
    net::IpAddr,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use developer_control::{
    camera::CameraPath,
    protocol::{Command, Condition, InputCommand, RecordSettings},
};
use serde_json::{Value, json};

/// Default reply wait for quick commands.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(30);
/// Encoding and muxing a long recording can take a while after `record_stop`.
pub const RECORD_STOP_TIMEOUT: Duration = Duration::from_secs(600);
/// Extra reply slack beyond a `wait_for` timeout.
const WAIT_SLACK: Duration = Duration::from_secs(5);
const DEFAULT_WAIT_MS: u64 = 60_000;

fn parse<T: serde::de::DeserializeOwned>(arguments: &Value, what: &str) -> Result<T, String> {
    serde_json::from_value(arguments.clone()).map_err(|error| format!("bad {what}: {error}"))
}

/// `name` relative to the checkout unless absolute.
pub fn resolve(repo: &Path, name: &str) -> PathBuf {
    let path = Path::new(name);
    if path.is_absolute() {
        path.to_owned()
    } else {
        repo.join(path)
    }
}

/// A fresh file under the control directory, e.g. `screenshots/1700000000123.png`.
pub fn generated(control_dir: &Path, folder: &str, extension: &str) -> PathBuf {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis());
    control_dir
        .join(folder)
        .join(format!("{millis}.{extension}"))
}

/// The command and reply timeout for a pass-through tool.
pub fn command(
    name: &str,
    arguments: &Value,
    repo: &Path,
    control_dir: &Path,
) -> Result<(Command, Duration), String> {
    Ok(match name {
        "input" => (
            Command::Input(parse::<InputCommand>(arguments, "input")?),
            CALL_TIMEOUT,
        ),
        "chat" => {
            let text = arguments
                .get("text")
                .and_then(Value::as_str)
                .ok_or("`text` is required")?;
            (
                Command::Chat {
                    text: text.to_owned(),
                },
                CALL_TIMEOUT,
            )
        }
        "camera_path" => {
            if arguments.get("release").and_then(Value::as_bool) == Some(true) {
                (Command::CameraRelease, CALL_TIMEOUT)
            } else {
                let path: CameraPath = parse(arguments, "camera path")?;
                path.validate()?;
                (Command::CameraPath(path), CALL_TIMEOUT)
            }
        }
        "test_cape" | "test_accounts" => {
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Toggle {
                enabled: bool,
            }
            let Toggle { enabled } = parse(arguments, name)?;
            let command = if name == "test_cape" {
                Command::TestCape { enabled }
            } else {
                Command::TestAccounts { enabled }
            };
            (command, CALL_TIMEOUT)
        }
        "state" => (Command::State, CALL_TIMEOUT),
        "wait_for" => {
            let condition: Condition = parse(
                arguments
                    .get("condition")
                    .ok_or("`condition` is required")?,
                "condition",
            )?;
            let timeout_ms = arguments
                .get("timeout_ms")
                .and_then(Value::as_u64)
                .unwrap_or(DEFAULT_WAIT_MS);
            (
                Command::WaitFor {
                    condition,
                    timeout_ms: Some(timeout_ms),
                },
                Duration::from_millis(timeout_ms) + WAIT_SLACK,
            )
        }
        "screenshot" => {
            let path = arguments.get("path").and_then(Value::as_str).map_or_else(
                || generated(control_dir, "screenshots", "png"),
                |path| resolve(repo, path),
            );
            (Command::Screenshot { path }, CALL_TIMEOUT)
        }
        "record_start" => {
            let mut arguments = arguments.clone();
            let path = arguments.get("path").and_then(Value::as_str).map_or_else(
                || generated(control_dir, "recordings", "mp4"),
                |path| resolve(repo, path),
            );
            arguments["path"] = json!(path);
            let settings: RecordSettings = parse(&arguments, "recording")?;
            if settings.fps == 0 {
                return Err("`fps` must be positive".into());
            }
            (Command::RecordStart(settings), CALL_TIMEOUT)
        }
        "record_stop" => (Command::RecordStop, RECORD_STOP_TIMEOUT),
        _ => return Err(format!("unknown tool `{name}`")),
    })
}

/// Whether `address` names this machine; remote joins need an explicit opt-in.
pub fn is_loopback(address: &str) -> bool {
    let host = match address.rsplit_once(':') {
        Some((host, port)) if port.chars().all(|c| c.is_ascii_digit()) => host,
        _ => address,
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    host.eq_ignore_ascii_case("localhost")
        || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}
