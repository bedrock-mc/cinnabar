//! Local, redacted diagnostics; raw account data is never added to an archive.
use crate::DevtoolError;
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
const PRIVATE_FIELDS: &str = "authorization|access[_-]?token|refresh[_-]?token|id[_-]?token|token|password|secret|xuid|gamertag|display[_-]?name|player[_-]?name|user[_-]?name|username|server[_-]?(?:address|host)?|target|address|hostname|host|from|to|owner[_-]?id|user[_-]?id|email|identity|socket[_-]?dir|endpoint";
const MAX_ESCAPE_DEPTH: usize = 4;

/// Decodes JSON and common debug escapes without requiring the surrounding log to be JSON.
fn decode_escapes(text: &str) -> String {
    static ESCAPES: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let escapes = ESCAPES.get_or_init(|| {
        Regex::new(r#"\\(?:u\{[0-9a-fA-F]{1,6}\}|x[0-9a-fA-F]{2}|u[0-9a-fA-F]{4}(?:\\u[0-9a-fA-F]{4})?|["\\/bfnrt])"#).unwrap()
    });
    escapes
        .replace_all(text, |capture: &regex::Captures<'_>| {
            let escaped = &capture[0];
            let hex = escaped.strip_prefix("\\x").or_else(|| {
                escaped
                    .strip_prefix("\\u{")
                    .and_then(|value| value.strip_suffix('}'))
            });
            if let Some(value) = hex
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                .and_then(char::from_u32)
            {
                value.to_string()
            } else {
                serde_json::from_str::<String>(&format!("\"{escaped}\""))
                    .unwrap_or_else(|_| escaped.into())
            }
        })
        .into_owned()
}

/// Retains raw and decoded views, rejecting bundles whose escape layers exceed the bound.
fn decoded_views(text: &str) -> Result<Vec<String>, DevtoolError> {
    let mut views = vec![text.to_owned()];
    for _ in 0..MAX_ESCAPE_DEPTH {
        let next = decode_escapes(views.last().unwrap());
        if next == *views.last().unwrap() {
            return Ok(views);
        }
        views.push(next);
    }
    if decode_escapes(views.last().unwrap()) != *views.last().unwrap() {
        return Err(DevtoolError::Usage(
            "diagnostic escape depth exceeds the redaction limit".into(),
        ));
    }
    Ok(views)
}

/// Builds literal spellings for learned values, including quote-only and nested JSON escaping.
fn encoded_values(values: Vec<String>) -> Vec<String> {
    let mut spellings = std::collections::BTreeSet::new();
    for value in values.into_iter().filter(|value| !value.is_empty()) {
        spellings.insert(value.replace('"', "\\\""));
        let mut escaped = value;
        for _ in 0..=MAX_ESCAPE_DEPTH {
            spellings.insert(escaped.clone());
            let json = serde_json::to_string(&escaped).unwrap();
            escaped = json[1..json.len() - 1].into();
        }
    }
    let mut values: Vec<_> = spellings.into_iter().collect();
    values.sort_by_key(|value| std::cmp::Reverse(value.len()));
    values
}

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
    let mut client = String::new();
    for name in ["client.log.1", "client.log"] {
        if let Ok(text) = tail(&root.join("logs").join(name)) {
            client.push_str(&text);
        }
    }
    let client = latest_session(&client, "CLIENT_SESSION_START");
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
    let session_start = Regex::new(r"timestamp_ms=(\d+)")
        .unwrap()
        .captures(client)
        .and_then(|capture| capture[1].parse::<u128>().ok());
    let mut reports: Vec<_> = fs::read_dir(root.join("crashes"))
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.path().extension().is_some_and(|ext| ext == "json")
                && session_start.is_none_or(|start| {
                    entry
                        .file_name()
                        .to_str()
                        .and_then(|name| name.strip_prefix("crash-"))
                        .and_then(|name| name.split('-').next())
                        .and_then(|stamp| stamp.parse::<u128>().ok())
                        .is_some_and(|stamp| stamp >= start)
                })
        })
        .collect();
    reports.sort_by_key(|entry| entry.file_name());
    for (index, entry) in reports.iter().rev().take(8).enumerate() {
        if fs::metadata(entry.path()).is_ok_and(|meta| meta.len() <= LOG_LIMIT)
            && let Ok(text) = fs::read_to_string(entry.path())
        {
            entries.push((format!("crashes/report-{index}.json"), text));
        }
    }
    let session_identity = client
        .lines()
        .find(|line| line.contains("CLIENT_SESSION_START"))
        .unwrap_or("session build identity unavailable");
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

/// Starts at the latest session marker, preserving rotation tails when the marker was evicted.
fn latest_session<'a>(text: &'a str, marker: &str) -> &'a str {
    text.rfind(marker).map_or(text, |start| &text[start..])
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
    let marker = regex::bytes::Regex::new(r"CORE_SESSION_START timestamp_ms=(\d+)\r?\n").unwrap();
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

/// Removes learned literal values before and after decoding, then scrubs contextual patterns.
fn redact(text: &str, values: &[String]) -> Result<String, DevtoolError> {
    let raw = redact_patterns(&redact_values(text, values));
    let decoded = decoded_views(&raw)?.pop().unwrap();
    Ok(redact_patterns(&redact_values(&decoded, values)))
}

/// Replaces every learned spelling before decoding can alter quote or backslash boundaries.
fn redact_values(text: &str, values: &[String]) -> String {
    let mut result = text.to_owned();
    for value in values {
        let pattern = Regex::new(&format!("(?i){}", regex::escape(value))).unwrap();
        result = pattern.replace_all(&result, "[redacted]").into_owned();
    }
    result
}

/// Applies credential and address rules to both raw and decoded text.
fn redact_patterns(text: &str) -> String {
    let mut result = text.to_owned();
    let contextual = format!(
        r#"(?i)\b({PRIVATE_FIELDS})\b["']?\s*[:=]\s*("(?:\\.|[^"\\\n])*"|'(?:\\.|[^'\\\n])*'|[^\s,}}\]]+)"#
    );
    let patterns = [
        contextual.as_str(),
        r"(?i)\bbearer\s+\S+",
        r#"(?i)\b[a-z][a-z0-9+.-]*://[^\s"<>]+"#,
        r"\b[A-Za-z0-9_-]{15,}\.[A-Za-z0-9_-]{15,}\.[A-Za-z0-9_-]+\b",
        r#"\b[^\s@"<>]+@[^\s@"<>]+\b"#,
        r"\b(?:\d{1,3}\.){3}\d{1,3}(?::\d+)?\b",
        r"(?i)\b[a-z][a-z0-9_.-]*:\d{2,5}\b",
        r"(?i)(?:\[)?(?:[a-f0-9]{0,4}:){2,}[a-f0-9:.%]+(?:\])?(?::\d+)?",
        r"\b\d{10,}\b",
        r"(?i)\b[a-z0-9](?:[a-z0-9_-]*\.)+[a-z]{2,}(?::\d+)?\b",
        r"(?i)(?:/Users/|/home/|[a-z]:\\Users\\)[^/\\\s]+",
    ];
    for pattern in patterns {
        result = Regex::new(pattern)
            .unwrap()
            .replace_all(&result, "[redacted]")
            .into_owned();
    }
    result
}

/// Learns identity and credential values from every raw and decoded view for bundle-wide removal.
fn sensitive_values(texts: &[String]) -> Vec<String> {
    let pattern = Regex::new(&format!(
        r#"(?i)\b(?:{PRIVATE_FIELDS})\b["']?\s*[:=]\s*("(?:\\.|[^"\\\n])*"|'(?:\\.|[^'\\\n])*'|[^\s,}}\]\[{{"']+)"#
    )).unwrap();
    let bearer = Regex::new(r#"(?i)\bbearer\s+([^\s"']+)"#).unwrap();
    let mut values = Vec::new();
    for text in texts {
        for capture in pattern.captures_iter(text) {
            let raw = &capture[1];
            let value = serde_json::from_str::<String>(raw)
                .unwrap_or_else(|_| raw.trim_matches(['"', '\'']).into());
            values.push(value);
        }
        values.extend(
            bearer
                .captures_iter(text)
                .map(|capture| capture[1].to_owned()),
        );
    }
    values
}

/// Collects values under private JSON fields, including values nested in arrays or objects.
fn json_sensitive_values(value: &serde_json::Value, values: &mut Vec<String>, private: bool) {
    match value {
        serde_json::Value::String(text) if private => values.push(text.clone()),
        serde_json::Value::Number(number) if private => values.push(number.to_string()),
        serde_json::Value::Array(items) => {
            for item in items {
                json_sensitive_values(item, values, private);
            }
        }
        serde_json::Value::Object(items) => {
            let fields = Regex::new(&format!("(?i)^(?:{PRIVATE_FIELDS})$")).unwrap();
            for (key, item) in items {
                json_sensitive_values(item, values, private || fields.is_match(key));
            }
        }
        _ => {}
    }
}

/// Scrubs decoded JSON values while preserving the report's structure and valid escaping.
fn redact_json(value: &mut serde_json::Value, names: &[String]) -> Result<(), DevtoolError> {
    match value {
        serde_json::Value::String(text) => *text = redact(text, names)?,
        serde_json::Value::Array(values) => {
            for value in values {
                redact_json(value, names)?;
            }
        }
        serde_json::Value::Object(values) => {
            let fields = Regex::new(&format!("(?i)^(?:{PRIVATE_FIELDS})$")).unwrap();
            for (key, mut value) in std::mem::take(values) {
                // The same contextual field rules apply to structured and plain-text values.
                if fields.is_match(decoded_views(&key)?.last().unwrap()) {
                    value = serde_json::Value::String("[redacted]".into());
                } else {
                    redact_json(&mut value, names)?;
                }
                let key = redact(&key, names)?;
                if values.insert(key, value).is_some() {
                    return Err(DevtoolError::Usage(
                        "redaction would merge diagnostic JSON keys".into(),
                    ));
                }
            }
        }
        _ => {}
    }
    Ok(())
}

/// Writes only scrubbed text using fixed member names and atomically publishes a completed zip.
fn bundle(path: &Path, entries: &[(String, String)]) -> Result<(), DevtoolError> {
    let mut reports = Vec::new();
    let mut names = Vec::new();
    for (name, text) in entries {
        names.extend(sensitive_values(&decoded_views(text)?));
        if name.starts_with("crashes/") && name.ends_with(".json") {
            let value: serde_json::Value = serde_json::from_str(text)?;
            json_sensitive_values(&value, &mut names, false);
            reports.push(Some(value));
        } else {
            reports.push(None);
        }
    }
    let names = encoded_values(names);
    let mut temporary = tempfile::NamedTempFile::new_in(path.parent().unwrap_or(Path::new(".")))
        .map_err(io_error)?;
    let mut archive = ZipWriter::new(temporary.as_file_mut());
    for ((name, text), report) in entries.iter().zip(reports) {
        let scrubbed = if let Some(mut value) = report {
            redact_json(&mut value, &names)?;
            serde_json::to_string_pretty(&value)?
        } else {
            redact(text, &names)?
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
mod tests {
    use super::*;
    #[test]
    fn excessive_escape_depth_never_publishes_a_bundle() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("bundle.zip");
        let mut text = "gamertag=\"Fixture Player\"".to_owned();
        for _ in 0..=MAX_ESCAPE_DEPTH {
            text = serde_json::to_string(&text).unwrap();
        }
        assert!(bundle(&path, &[("client.log".into(), text)]).is_err());
        assert!(!path.exists());
    }

    #[test]
    fn escaped_payloads_remove_learned_secrets_and_unlabelled_identities_everywhere() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("bundle.zip");
        let secret = "FIXTURE_PRIVATE_TOKEN\"with\\slash";
        let name = "Escaped \"Fixture\" Player";
        let payload = serde_json::json!({"access_token": secret, "gamertag": name}).to_string();
        let escaped = serde_json::to_string(&payload).unwrap();
        let double = serde_json::to_string(&escaped).unwrap();
        let unicode: String = name
            .chars()
            .map(|c| format!("\\u{:04x}", c as u32))
            .collect();
        bundle(&path, &[
            ("client.log".into(), format!("error={escaped}\nerror={double}\nlater {name} connected\n")),
            ("core.log".into(), format!("unlabelled {secret}\nencoded {}\nunicode {unicode}\n", serde_json::to_string(name).unwrap())),
            ("crashes/report-0.json".into(), serde_json::json!({"message": double, "later": format!("{name} connected {secret}"), (name): "connected"}).to_string()),
        ]).unwrap();
        let mut zip = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
        let mut all = String::new();
        for index in 0..zip.len() {
            zip.by_index(index)
                .unwrap()
                .read_to_string(&mut all)
                .unwrap();
        }
        assert!(all.contains("connected"));
        for private in ["FIXTURE_PRIVATE_TOKEN", "Escaped", "Fixture", &unicode] {
            assert!(!all.contains(private), "leaked fixture value: {all}");
        }
    }

    #[test]
    fn rotated_core_logs_keep_all_current_session_events() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("core.log");
        fs::write(path.with_file_name("core.log.1"), "CORE_SESSION_START timestamp_ms=1\nstale\nCORE_SESSION_START timestamp_ms=3\nupstream first elapsed_ms=4\n").unwrap();
        fs::write(
            &path,
            "CORE_SESSION_START timestamp_ms=5\nupstream second elapsed_ms=6\n",
        )
        .unwrap();
        let text = core_session_tail(&path, "CLIENT_SESSION_START timestamp_ms=2\n").unwrap();
        assert!(text.contains("first"), "lost earlier core run: {text}");
        assert!(text.contains("second"));
        assert!(!text.contains("stale"));
        let mut current = fs::OpenOptions::new().append(true).open(&path).unwrap();
        current.write_all(&vec![b'x'; LOG_LIMIT as usize]).unwrap();
        current
            .write_all(b"\nupstream tail elapsed_ms=7\n")
            .unwrap();
        drop(current);
        let bounded = core_session_tail(&path, "CLIENT_SESSION_START timestamp_ms=2\n").unwrap();
        assert_eq!(bounded.len() as u64, LOG_LIMIT);
        assert!(bounded.contains("upstream tail"));
        assert!(!bounded.contains("first"));
        fs::remove_file(&path).unwrap();
        assert!(
            core_session_tail(&path, "CLIENT_SESSION_START timestamp_ms=2\n")
                .unwrap()
                .contains("first")
        );
    }

    #[test]
    fn crash_json_strings_are_decoded_redacted_and_serialized() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("bundle.zip");
        let report = serde_json::json!({"log_tail": "gamertag=\"Private Player\" token=\"secret value\"", "nested": [{"message": "Private Player connected"}], "gamertag": "Other Player"});
        bundle(
            &path,
            &[
                ("crashes/report-0.json".into(), report.to_string()),
                ("core.log".into(), "Other Player connected".into()),
            ],
        )
        .unwrap();
        let mut zip = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
        let mut text = String::new();
        zip.by_name("crashes/report-0.json")
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert!(
            value["nested"][0]["message"]
                .as_str()
                .unwrap()
                .contains("connected")
        );
        for secret in ["Private", "Player", "secret", "value", "Other"] {
            assert!(!text.contains(secret), "leaked {secret}");
        }
        text.clear();
        zip.by_name("core.log")
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        assert!(!text.contains("Other Player"));
    }

    #[test]
    fn long_current_core_session_keeps_its_tail_and_join_timeline() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("logs")).unwrap();
        fs::write(
            temp.path().join("logs/client.log"),
            "CLIENT_SESSION_START timestamp_ms=2\n",
        )
        .unwrap();
        let mut core = fs::File::create(temp.path().join("logs/core.log")).unwrap();
        core.write_all(
            b"CORE_SESSION_START timestamp_ms=1\nstale\nCORE_SESSION_START timestamp_ms=3\n",
        )
        .unwrap();
        for _ in 0..(LOG_LIMIT / 128 + 1) {
            writeln!(core, "{}", "x".repeat(128)).unwrap();
        }
        core.write_all(b"upstream StartGame elapsed_ms=12\n")
            .unwrap();
        drop(core);
        run(&["--data-root".into(), temp.path().display().to_string()]).unwrap();
        let path = fs::read_dir(temp.path().join("diag"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let mut zip = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
        for name in ["core.log", "join-timeline.txt"] {
            let mut text = String::new();
            zip.by_name(name)
                .unwrap()
                .read_to_string(&mut text)
                .unwrap();
            assert!(text.contains("StartGame"), "missing {name}");
            assert!(!text.contains("stale"));
            assert!(text.len() as u64 <= LOG_LIMIT);
        }
    }

    #[test]
    fn archive_contains_only_redacted_members_including_repeated_names() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("bundle.zip");
        let entries = vec![("client.log".into(), r#"gamertag="Private Player" email=alice@example.invalid XUID=2535400000000000 token="secret value" target=private-host:19132 addr=2001:db8::1 url=https://play.example.invalid/path 192.168.1.2:19132"#.into()), ("core.log".into(), "Private Player connected".into())];
        bundle(&path, &entries).unwrap();
        let mut zip = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
        let mut result = String::new();
        for index in 0..zip.len() {
            zip.by_index(index)
                .unwrap()
                .read_to_string(&mut result)
                .unwrap();
        }
        for secret in [
            "Private Player",
            "alice",
            "2535400000000000",
            "secret value",
            "private-host",
            "2001:db8",
            "play.example",
            "192.168",
        ] {
            assert!(!result.contains(secret), "leaked {secret}: {result}");
        }
        assert!(result.contains("connected"));
    }
    #[test]
    fn newest_session_excludes_previous_launches() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("core.log");
        let recent_core_session = |core: &str, client: &str| {
            fs::write(&path, core).unwrap();
            core_session_tail(&path, client).unwrap()
        };
        let text = "old\nCLIENT_SESSION_START timestamp_ms=1\nfirst\nCLIENT_SESSION_START timestamp_ms=2\nlast\n";
        assert_eq!(
            latest_session(text, "CLIENT_SESSION_START"),
            "CLIENT_SESSION_START timestamp_ms=2\nlast\n"
        );
        assert_eq!(
            recent_core_session(
                "CORE_SESSION_START timestamp_ms=1\nold\nCORE_SESSION_START timestamp_ms=3\nnew",
                latest_session(text, "CLIENT_SESSION_START")
            ),
            "CORE_SESSION_START timestamp_ms=3\nnew"
        );
        assert_eq!(
            recent_core_session(
                "CORE_SESSION_START timestamp_ms=1\nold",
                "CLIENT_SESSION_START timestamp_ms=2\ncurrent"
            ),
            ""
        );
        assert_eq!(
            recent_core_session(
                "CORE_SESSION_START timestamp_ms=1\nold",
                "legacy client log"
            ),
            "CORE_SESSION_START timestamp_ms=1\nold"
        );
    }
}
