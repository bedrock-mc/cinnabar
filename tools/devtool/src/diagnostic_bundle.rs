//! Local diagnostics redacted by `crash_report::redact`; raw account data is never added to an
//! archive.
use crate::DevtoolError;
use crash_report::{folder, redact, section};
use regex::Regex;
use std::{
    fs,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};
use zip::{ZipWriter, write::SimpleFileOptions};
const LOG_LIMIT: u64 = 8 << 20;
/// Bundles the latest local session without uploading any data.
pub(crate) fn run(args: &[String]) -> Result<(), DevtoolError> {
    let root = match args {
        [] => std::env::var_os("CINNABAR_USER_ROOT").map_or_else(
            || PathBuf::from(".local"),
            |root| PathBuf::from(root).join("data"),
        ),
        [flag, path] if flag == "--data-root" => PathBuf::from(path),
        _ => return Err(DevtoolError::Usage("diag [--data-root <path>]".into())),
    };
    let mut entries = Vec::new();
    // `client.log` holds only the current session; earlier ones are archived beside it, and a
    // session that outgrew its file repeats the header at the top of the current one.
    let client = tail(&root.join("logs/client.log")).unwrap_or_default();
    let client = section::latest(&client, section::CLIENT_SESSION_START);
    entries.push(("client.log".to_owned(), client.to_owned()));
    let core = core_session_tail(&root.join("logs/core.log"), client).unwrap_or_default();
    let timeline = core
        .lines()
        .filter(|line| {
            line.contains("upstream ")
                && (line.contains("elapsed_ms")
                    || line.contains("connection starting")
                    || line.contains("connection failed"))
        })
        .collect::<Vec<_>>()
        .join("\n");
    entries.push(("core.log".into(), core));
    entries.push(("join-timeline.txt".into(), timeline));
    let session_identity = client
        .lines()
        .find(|line| line.contains(section::CLIENT_SESSION_START));
    for crash in session_crashes(&root.join("crashes"), session_identity) {
        let Some(name) = crash.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        for member in [
            folder::REPORT_FILE,
            folder::CLIENT_LOG_FILE,
            folder::CORE_LOG_FILE,
        ] {
            let path = crash.join(member);
            if fs::metadata(&path).is_ok_and(|meta| meta.len() <= LOG_LIMIT)
                && let Ok(text) = fs::read_to_string(&path)
            {
                entries.push((format!("crashes/{name}/{member}"), text));
            }
        }
    }
    let session_identity = session_identity.unwrap_or("session build identity unavailable");
    entries.push((
        "build.txt".into(),
        format!(
            "{session_identity}\ncheckout_commit={}\ndevtool_version={}\n",
            capture("git", &["rev-parse", "HEAD"]),
            env!("CARGO_PKG_VERSION")
        ),
    ));
    entries.push(("system.txt".into(), system_summary()));
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let directory = root.join("diag");
    fs::create_dir_all(&directory).map_err(io_error)?;
    let path = directory.join(format!("diag-{timestamp}.zip"));
    bundle(&path, &entries)?;
    println!("{}", path.display());
    Ok(())
}

/// The newest crash folders of the session whose header is `header`, newest first.
///
/// A header with a `session=` id selects its reports; an older header without one selects
/// folders created no earlier than its `timestamp_ms` second.
fn session_crashes(crash_dir: &Path, header: Option<&str>) -> Vec<PathBuf> {
    let id = header.and_then(|line| header_field(line, "session"));
    let start = header
        .and_then(|line| header_field(line, "timestamp_ms"))
        .and_then(|value| value.parse::<u64>().ok());
    let mut crashes: Vec<PathBuf> = folder::list(crash_dir)
        .into_iter()
        .filter(|crash| match id {
            Some(id) => folder::read(crash).is_ok_and(|report| report.session_id == id),
            None => start.is_none_or(|start| {
                folder::created_at_ms(crash).is_some_and(|created| created + 1000 > start)
            }),
        })
        .collect();
    crashes.reverse();
    crashes.truncate(folder::MAX_REPORTS);
    crashes
}

/// The value of a whitespace-separated `key=value` field on a header line.
fn header_field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    line.split_whitespace()
        .find_map(|field| field.strip_prefix(key)?.strip_prefix('='))
}

/// Scans both core-log generations for the client boundary, then reads a bounded combined tail.
fn core_session_tail(path: &Path, client: &str) -> std::io::Result<String> {
    let mut files = Vec::new();
    for generation in [path.with_extension("log.1"), path.to_owned()] {
        match fs::File::open(generation) {
            Ok(file) => files.push(file),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    let timestamp = Regex::new(r"timestamp_ms=(\d+)")
        .unwrap()
        .captures(client)
        .and_then(|capture| capture[1].parse::<u128>().ok());
    let marker = regex::bytes::Regex::new(&format!(
        r"{} timestamp_ms=(\d+)\r?\n",
        section::CORE_SESSION_START
    ))
    .unwrap();
    let mut buffer = [0u8; 64 << 10];
    let mut carry = Vec::new();
    let mut offset = 0u64;
    let mut session = None;
    for file in &mut files {
        loop {
            let count = file.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            let start = offset.saturating_sub(carry.len() as u64);
            carry.extend_from_slice(&buffer[..count]);
            for capture in marker.captures_iter(&carry) {
                let Some(at) = std::str::from_utf8(&capture[1])
                    .ok()
                    .and_then(|stamp| stamp.parse::<u128>().ok())
                else {
                    continue;
                };
                if timestamp.is_none()
                    || (session.is_none() && timestamp.is_some_and(|client_at| at >= client_at))
                {
                    session = Some(start + capture.get(0).unwrap().start() as u64);
                }
            }
            offset += count as u64;
            // Every emitted marker fits in this overlap, including a boundary between read blocks.
            if carry.len() > 256 {
                carry.drain(..carry.len() - 256);
            }
        }
    }
    let Some(session) = session.or_else(|| timestamp.is_none().then_some(0)) else {
        return Ok(String::new());
    };
    let start = session.max(offset.saturating_sub(LOG_LIMIT));
    let mut bytes = Vec::new();
    let mut generation_start = 0;
    for mut file in files {
        let length = file.stream_position()?;
        let relative_start = start.saturating_sub(generation_start).min(length);
        file.seek(SeekFrom::Start(relative_start))?;
        (&mut file)
            .take((length - relative_start).min(LOG_LIMIT - bytes.len() as u64))
            .read_to_end(&mut bytes)?;
        generation_start += length;
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Reads a bounded tail; malformed UTF-8 becomes replacement characters.
fn tail(path: &Path) -> std::io::Result<String> {
    let mut file = fs::File::open(path)?;
    let length = file.metadata()?.len();
    file.seek(SeekFrom::Start(length.saturating_sub(LOG_LIMIT)))?;
    let mut bytes = Vec::new();
    file.take(LOG_LIMIT).read_to_end(&mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Writes only scrubbed text using fixed member names and atomically publishes a completed zip.
fn bundle(path: &Path, entries: &[(String, String)]) -> Result<(), DevtoolError> {
    let mut reports = Vec::new();
    let mut learned = redact::Learned::default();
    for (name, text) in entries {
        learned.text(text).map_err(redaction_error)?;
        if name.starts_with("crashes/") && name.ends_with(".json") {
            let value: serde_json::Value = serde_json::from_str(text)?;
            learned.json(&value);
            reports.push(Some(value));
        } else {
            reports.push(None);
        }
    }
    let redactor = learned.redactor();
    let mut temporary = tempfile::NamedTempFile::new_in(path.parent().unwrap_or(Path::new(".")))
        .map_err(io_error)?;
    let mut archive = ZipWriter::new(temporary.as_file_mut());
    for ((name, text), report) in entries.iter().zip(reports) {
        let scrubbed = if let Some(mut value) = report {
            redactor.json(&mut value).map_err(redaction_error)?;
            serde_json::to_string_pretty(&value)?
        } else {
            redactor.text(text).map_err(redaction_error)?
        };
        archive
            .start_file(
                name,
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
            )
            .map_err(|error| DevtoolError::Usage(error.to_string()))?;
        archive.write_all(scrubbed.as_bytes()).map_err(io_error)?;
    }
    archive
        .finish()
        .map_err(|error| DevtoolError::Usage(error.to_string()))?;
    temporary
        .persist_noclobber(path)
        .map_err(|error| io_error(error.error))?;
    Ok(())
}

/// Refuses to publish a bundle that could not be redacted safely.
fn redaction_error(error: redact::Error) -> DevtoolError {
    DevtoolError::Usage(format!("diagnostic bundle not written: {error}"))
}

/// Converts local filesystem failures into the command error type.
fn io_error(source: std::io::Error) -> DevtoolError {
    DevtoolError::Spawn {
        command: "diagnostic bundle".into(),
        source,
    }
}

/// Captures a specific non-secret system command without inspecting the environment.
fn capture(program: &str, args: &[&str]) -> String {
    Command::new(program)
        .args(args)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map_or_else(
            || "unavailable".into(),
            |output| String::from_utf8_lossy(&output.stdout).trim().into(),
        )
}

/// Reports OS and GPU model fields, excluding machine names and serial numbers.
fn system_summary() -> String {
    let os = capture("uname", &["-srm"]);
    let gpu = if cfg!(target_os = "macos") {
        let raw = capture("system_profiler", &["SPDisplaysDataType", "-json"]);
        let value: serde_json::Value = serde_json::from_str(&raw).unwrap_or_default();
        value["SPDisplaysDataType"].as_array().map_or_else(
            || "unavailable".into(),
            |devices| {
                devices
                    .iter()
                    .map(|device| {
                        format!(
                            "{} vendor={} cores={}",
                            device["sppci_model"],
                            device["spdisplays_vendor"],
                            device["sppci_cores"]
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            },
        )
    } else if cfg!(windows) {
        capture(
            "powershell",
            &[
                "-NoProfile",
                "-Command",
                "Get-CimInstance Win32_VideoController | Select-Object -ExpandProperty Name",
            ],
        )
    } else {
        capture("lspci", &[])
            .lines()
            .filter(|line| {
                line.contains("VGA")
                    || line.contains("3D controller")
                    || line.contains("Display controller")
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "os={} arch={}\n{os}\ngpu={gpu}\n",
        std::env::consts::OS,
        std::env::consts::ARCH
    )
}

#[cfg(test)]
mod tests;
