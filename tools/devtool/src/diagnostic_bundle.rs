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
/// An authorization value of any scheme: quoted whole, or unquoted as the scheme plus one credential.
const AUTHORIZATION: &str = r#"(?i)\b(?:proxy-)?authorization\b["']?\s*[:=]\s*("(?:\\.|[^"\\\n])*"|'(?:\\.|[^'\\\n])*'|[^\s"',}\]\[{:][^\s"',}\]]*(?:[ \t]+[^\s"',}\]]+)?)"#;
/// An Xbox Live authorization value, which is sensitive even without a label.
const XBL_TOKEN: &str = r#"XBL3\.0 (x=[^\s;"]+;[^\s"]+)"#;
/// A home folder prefix and its username; a name with spaces runs to the next separator or quote,
/// and an apostrophe followed by a name character stays part of the name.
const HOME_FOLDER: &str = r#"(?i)(?:/Users/|/home/|[a-z]:\\Users\\)(?:([^/\\\r\n"'<>|:*?,=]+(?:'[^/\\\s"'<>|:*?,=][^/\\\r\n"'<>|:*?,=]*)*)(?:[\\/"]|'(?:$|[/\\\s"'<>|:*?,=]))|([^/\\\s"']+(?:'[^/\\\s"']+)*))"#;

/// Removes ANSI styling, which a terminal formatter can place between a field name and `=`.
fn strip_ansi(text: &str) -> String {
    static ANSI: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    ANSI.get_or_init(|| Regex::new(r"\x1b\[[0-?]*[ -/]*[@-~]").unwrap())
        .replace_all(text, "")
        .into_owned()
}

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
    let mut views = vec![strip_ansi(text)];
    for _ in 0..MAX_ESCAPE_DEPTH {
        let next = strip_ansi(&decode_escapes(views.last().unwrap()));
        if next == *views.last().unwrap() {
            return Ok(views);
        }
        views.push(next);
    }
    if decode_escapes(views.last().unwrap()) != *views.last().unwrap() {
        return Err(escape_depth_error());
    }
    Ok(views)
}

/// Rejects text nested deeper than redaction decodes.
fn escape_depth_error() -> DevtoolError {
    DevtoolError::Usage("diagnostic escape depth exceeds the redaction limit".into())
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

/// Scrubs learned values and contextual patterns from the raw text and every decoded layer.
///
/// Emits the shallowest layer that needed no further redaction, so literal backslashes such as
/// Windows paths survive unless a deeper decode revealed something private.
fn redact(text: &str, values: &[String]) -> Result<String, DevtoolError> {
    let scrub = |view: &str| redact_patterns(&redact_values(view, values));
    let mut layer = scrub(&strip_ansi(text));
    let mut emitted = layer.clone();
    for _ in 0..MAX_ESCAPE_DEPTH {
        let decoded = strip_ansi(&decode_escapes(&layer));
        if decoded == layer {
            return Ok(emitted);
        }
        // A later decode can disguise a value, such as turning `\b` into a backspace.
        layer = scrub(&decoded);
        if layer != decoded {
            emitted = layer.clone();
        }
    }
    if strip_ansi(&decode_escapes(&layer)) != layer {
        return Err(escape_depth_error());
    }
    Ok(emitted)
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
        r#"(?i)\b({PRIVATE_FIELDS})\b["']?\s*[:=]\s*("(?:\\.|[^"\\\n])*"|'(?:\\.|[^'\\\n])*'|[^\s,}}\]:][^\s,}}\]]*)"#
    );
    let patterns = [
        AUTHORIZATION,
        XBL_TOKEN,
        contextual.as_str(),
        r"(?i)\bbearer\s+\S+",
        r#"(?i)\b[a-z][a-z0-9+.-]*://[^\s"<>]+"#,
        r"\b[A-Za-z0-9_-]{15,}\.[A-Za-z0-9_-]{15,}\.[A-Za-z0-9_-]+\b",
        r#"\b[^\s@"<>]+@[^\s@"<>]+\b"#,
    ];
    for pattern in patterns {
        result = Regex::new(pattern)
            .unwrap()
            .replace_all(&result, "[redacted]")
            .into_owned();
    }
    result = redact_ipv6(&result);
    result = Regex::new(r"\b(?:\d{1,3}\.){3}\d{1,3}(?::\d+)?\b")
        .unwrap()
        .replace_all(&result, "[redacted]")
        .into_owned();
    for pattern in [
        r"(?i)\b[a-z][a-z0-9_.-]*:\d{2,5}\b",
        r"\b\d{10,}\b",
        r"(?i)\b[a-z0-9](?:[a-z0-9_-]*\.)+[a-z]{2,}(?::\d+)?\b",
    ] {
        let sources = source_locations(&result);
        result = Regex::new(pattern)
            .unwrap()
            .replace_all(&result, |capture: &regex::Captures<'_>| {
                let span = capture.get(0).unwrap().range();
                if sources
                    .iter()
                    .any(|source| source.start <= span.start && span.end <= source.end)
                {
                    capture[0].to_owned()
                } else {
                    "[redacted]".to_owned()
                }
            })
            .into_owned();
    }
    Regex::new(HOME_FOLDER)
        .unwrap()
        .replace_all(&result, |capture: &regex::Captures<'_>| {
            let whole = capture.get(0).unwrap();
            let name = capture.get(1).or(capture.get(2)).unwrap();
            format!(
                "[redacted]{}",
                &whole.as_str()[name.end() - whole.start()..]
            )
        })
        .into_owned()
}

/// Byte ranges of `<file>.rs:<line>` (or `.go`) source locations after a path separator or with a column.
fn source_locations(text: &str) -> Vec<std::ops::Range<usize>> {
    Regex::new(r"(?i)[\w.-]+\.(?:rs|go):\d+(:\d+)?")
        .unwrap()
        .captures_iter(text)
        .filter(|capture| {
            capture.get(1).is_some()
                || text[..capture.get(0).unwrap().start()].ends_with(['/', '\\'])
        })
        .map(|capture| capture.get(0).unwrap().range())
        .collect()
}

/// Redacts standalone IPv6 addresses, leaving `::`-joined symbols and `h:m:s` times intact.
fn redact_ipv6(text: &str) -> String {
    let candidate =
        Regex::new(r"(?i)\[?(?:[0-9a-f]{0,4}:){2,}[0-9a-f.]*(?:%[0-9a-z]+)?\]?(?::\d{1,5})?")
            .unwrap();
    let identifier = |c: char| c.is_alphanumeric() || c == '_';
    // `<u32 as Add>::add` and `{{closure}}::fade` are Rust paths that also parse as addresses.
    let path = |c: char| identifier(c) || matches!(c, '>' | '}' | ')' | ':');
    candidate
        .replace_all(text, |capture: &regex::Captures<'_>| {
            let span = capture.get(0).unwrap();
            let mut address = span.as_str().trim_end_matches('.');
            // A prose colon after an address, as in `via ::1: timeout`, is not part of it.
            if !ipv6_address(address) && address.ends_with(':') && !address.ends_with("::") {
                address = &address[..address.len() - 1];
            }
            let standalone = !text[..span.start()].ends_with(path)
                && !text[span.start() + address.len()..].starts_with(identifier);
            if standalone && ipv6_address(address) {
                format!("[redacted]{}", &span.as_str()[address.len()..])
            } else {
                span.as_str().to_owned()
            }
        })
        .into_owned()
}

/// Whether a candidate, with optional brackets, zone, and port, is an address with a hex group.
fn ipv6_address(candidate: &str) -> bool {
    let mut addresses = Vec::new();
    match candidate.strip_prefix('[') {
        Some(rest) => addresses.push(rest.split_once(']').map_or(rest, |(address, _)| address)),
        None => {
            addresses.push(candidate);
            if let Some((address, port)) = candidate.rsplit_once(':')
                && !port.is_empty()
                && port.bytes().all(|byte| byte.is_ascii_digit())
            {
                addresses.push(address);
            }
        }
    }
    addresses.into_iter().any(|address| {
        let address = address
            .split_once('%')
            .map_or(address, |(address, _)| address);
        address.parse::<std::net::Ipv6Addr>().is_ok()
            && address.split(':').any(|group| !group.is_empty())
    })
}

/// Learns identity and credential values from every raw and decoded view for bundle-wide removal.
fn sensitive_values(texts: &[String]) -> Vec<String> {
    let pattern = Regex::new(&format!(
        r#"(?i)\b(?:{PRIVATE_FIELDS})\b["']?\s*[:=]\s*("(?:\\.|[^"\\\n])*"|'(?:\\.|[^'\\\n])*'|[^\s,}}\]\[{{"':][^\s,}}\]\[{{"']*)"#
    )).unwrap();
    let bearer = Regex::new(r#"(?i)\bbearer\s+([^\s"']+)"#).unwrap();
    let home = Regex::new(HOME_FOLDER).unwrap();
    let authorization = Regex::new(AUTHORIZATION).unwrap();
    let xbl = Regex::new(XBL_TOKEN).unwrap();
    let mut values = Vec::new();
    for text in texts {
        embedded_json_values(text, &mut values);
        for capture in authorization
            .captures_iter(text)
            .chain(xbl.captures_iter(text))
        {
            let raw = capture.get(1).unwrap().as_str();
            if raw.ends_with('\\') {
                continue;
            }
            let value = serde_json::from_str::<String>(raw)
                .unwrap_or_else(|_| raw.trim_matches(['"', '\'']).into());
            if let Some((_, credential)) = value.trim().split_once([' ', '\t']) {
                values.push(credential.trim().to_owned());
            }
            values.push(value);
        }
        values.extend(home.captures_iter(text).filter_map(|capture| {
            capture
                .get(1)
                .or(capture.get(2))
                .map(|name| name.as_str().to_owned())
        }));
        for capture in pattern.captures_iter(text) {
            let raw = &capture[1];
            // A trailing backslash is a cut escape; the decoded view learns the whole value.
            if raw.ends_with('\\') {
                continue;
            }
            let value = serde_json::from_str::<String>(raw)
                .unwrap_or_else(|_| raw.trim_matches(['"', '\'']).into());
            values.push(value);
        }
        values.extend(
            bearer
                .captures_iter(text)
                .map(|capture| capture[1].to_owned())
                .filter(|value| !value.ends_with('\\')),
        );
    }
    values
}

/// Learns private values from JSON objects and arrays embedded in log text or messages.
fn embedded_json_values(text: &str, values: &mut Vec<String>) {
    let mut start = 0;
    while let Some(offset) = text[start..].find(['{', '[']) {
        let at = start + offset;
        let mut stream =
            serde_json::Deserializer::from_str(&text[at..]).into_iter::<serde_json::Value>();
        start = match stream.next() {
            Some(Ok(value)) => {
                json_sensitive_values(&value, values, false);
                at + stream.byte_offset()
            }
            _ => at + 1,
        };
    }
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
mod tests;
