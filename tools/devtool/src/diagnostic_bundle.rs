//! Local diagnostics redacted by `crash_report::redact`; raw account data is never added to an
//! archive.
use crate::DevtoolError;
use crash_report::{folder, redact, section};
use diagnostics::session_log::{Family, Retention};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Seek, SeekFrom, Write},
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
    let client = client_session_logs(&root.join("logs")).unwrap_or_default();
    let client = client.as_str();
    entries.push(("client.log".to_owned(), client.to_owned()));
    let core = core_session_tail(&root.join("logs/core.log"), client).unwrap_or_default();
    let timeline = join_timeline(client, &core);
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
            folder::PACKETS_FILE,
            folder::FLIGHT_LOG_FILE,
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

/// Keeps client timing records and the matching core milestones in the diagnostic bundle.
fn join_timeline(client: &str, core: &str) -> String {
    client
        .lines()
        .filter(|line| line.contains("join_profile_id="))
        .chain(core.lines().filter(|line| {
            line.contains("join_profile_id=")
                || (line.contains("upstream ")
                    && (line.contains("elapsed_ms")
                        || line.contains("connection starting")
                        || line.contains("connection failed")))
        }))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Collects the current session's retained log segments, oldest first, with a bounded tail
/// per file and at most the configured number of archives. Repeated headers identify segments
/// from the same session; logs without a header cannot safely select older files.
fn client_session_logs(log_dir: &Path) -> std::io::Result<String> {
    let family = Family::new(log_dir, "client", "log");
    let current = session_segment(&family.current(), section::CLIENT_SESSION_START)?;
    let current = section::latest(&current, section::CLIENT_SESSION_START);
    let header = current
        .lines()
        .find(|line| line.contains(section::CLIENT_SESSION_START));
    let Some(header) = header else {
        return Ok(current.to_owned());
    };
    let archives = family.archives()?;
    let mut combined = String::new();
    for path in archives
        .iter()
        .skip(archives.len().saturating_sub(Retention::DEFAULT.archives))
    {
        let Ok(text) = session_segment(path, section::CLIENT_SESSION_START) else {
            continue;
        };
        let segment = section::latest(&text, section::CLIENT_SESSION_START);
        if segment.lines().next() == Some(header) {
            combined.push_str(segment);
            if !combined.ends_with('\n') {
                combined.push('\n');
            }
        }
    }
    combined.push_str(current);
    Ok(combined)
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

/// Reads retained core segments in archive order, selecting the client's identity when present
/// and using timestamps for legacy logs. The combined result is bounded across all files.
fn core_session_tail(path: &Path, client: &str) -> std::io::Result<String> {
    let family = Family::new(path.parent().unwrap_or(Path::new(".")), "core", "log");
    let archives = family.archives()?;
    let archive_start = archives.len().saturating_sub(Retention::DEFAULT.archives);
    let paths = std::iter::once(path.with_extension("log.1"))
        .chain(archives.into_iter().skip(archive_start))
        .chain(std::iter::once(path.to_owned()));
    let header = client
        .lines()
        .find(|line| line.contains(section::CLIENT_SESSION_START));
    let id = header.and_then(|line| header_field(line, "session"));
    let timestamp = header
        .and_then(|line| header_field(line, "timestamp_ms"))
        .and_then(|stamp| stamp.parse::<u64>().ok());
    let marker = regex::bytes::Regex::new(&format!(
        r"(?m)^{} [^\r\n]*\r?\n",
        section::CORE_SESSION_START
    ))
    .unwrap();
    let mut files = Vec::new();
    let mut sections = Vec::new();
    for path in paths {
        let mut file = match fs::File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        let mut headers = std::collections::BTreeMap::new();
        let mut buffer = [0; 64 << 10];
        let mut carry = Vec::new();
        let mut offset = 0;
        loop {
            let count = file.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            let start = offset - carry.len() as u64;
            carry.extend_from_slice(&buffer[..count]);
            for matched in marker.find_iter(&carry) {
                let line = String::from_utf8_lossy(matched.as_bytes());
                let matches = match (id, header_field(&line, "session")) {
                    (Some(wanted), Some(actual)) => wanted == actual,
                    _ => timestamp.is_none_or(|start| {
                        header_field(&line, "timestamp_ms")
                            .and_then(|stamp| stamp.parse::<u64>().ok())
                            .is_some_and(|at| at >= start)
                    }),
                };
                headers.insert(start + matched.start() as u64, matches);
            }
            offset += count as u64;
            if carry.len() > 256 {
                carry.drain(..carry.len() - 256);
            }
        }
        let mut selected = None;
        for (at, matches) in headers {
            if let Some(start) = selected.take() {
                sections.push((files.len(), start, at));
            }
            if matches {
                selected = Some(at);
            }
            if id.is_none() && timestamp.is_none() {
                sections.clear();
            }
        }
        if let Some(start) = selected {
            sections.push((files.len(), start, offset));
        } else if id.is_none() && timestamp.is_none() {
            sections.push((files.len(), 0, offset));
        }
        files.push(file);
    }
    let total: u64 = sections.iter().map(|(_, start, end)| end - start).sum();
    let mut skip = total.saturating_sub(LOG_LIMIT);
    let mut bytes = Vec::new();
    for (index, start, end) in sections {
        let skipped = skip.min(end - start);
        skip -= skipped;
        let start = start + skipped;
        let file = &mut files[index];
        file.seek(SeekFrom::Start(start))?;
        file.take(end - start).read_to_end(&mut bytes)?;
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Keeps the latest session header even when it falls outside a large segment's tail.
fn session_segment(path: &Path, marker: &str) -> std::io::Result<String> {
    let mut file = fs::File::open(path)?;
    let length = file.metadata()?.len();
    file.seek(SeekFrom::Start(length.saturating_sub(LOG_LIMIT)))?;
    let mut bytes = Vec::new();
    (&mut file)
        .take(length.min(LOG_LIMIT))
        .read_to_end(&mut bytes)?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    if !text.lines().any(|line| line.contains(marker)) {
        file.rewind()?;
        // Match the tail's snapshot: later appends must not change its session identity.
        if let Some(header) = latest_header(file.take(length), marker)? {
            return Ok(format!("{header}\n{text}"));
        }
    }
    Ok(text)
}

/// Finds the newest line-start header with bounded line prefixes, skipping long payload lines.
/// A header exceeding the shared header bound clears any older identity.
fn latest_header(file: impl Read, marker: &str) -> std::io::Result<Option<String>> {
    let mut reader = BufReader::new(file);
    let mut line = Vec::new();
    let mut latest = None;
    loop {
        line.clear();
        let count = reader
            .by_ref()
            .take(section::HEADER_BYTES as u64 + 1)
            .read_until(b'\n', &mut line)?;
        if count == 0 {
            break;
        }
        if line.starts_with(marker.as_bytes())
            && line.get(marker.len()).is_none_or(u8::is_ascii_whitespace)
        {
            latest = (line.len() <= section::HEADER_BYTES).then(|| {
                String::from_utf8_lossy(&line)
                    .trim_end_matches(['\r', '\n'])
                    .to_owned()
            });
        }
        if !line.ends_with(b"\n") {
            reader.skip_until(b'\n')?;
        }
    }
    Ok(latest)
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
