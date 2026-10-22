//! What an opted-in crash upload sends: one crash folder reduced to a redacted, size-capped
//! payload with a grouping signature. Sending and consent belong to the caller; this module
//! never touches the network or the clock.
//!
//! The payload carries no session id, user comment, minidump, packets or decode samples, and
//! every free-text field passes the shared redaction rules after learning private values from
//! the whole crashing session's log.

mod signature;
pub mod state;
#[cfg(test)]
mod tests;

use std::{
    fs,
    io::{self, Read, Write},
    path::Path,
};

use flate2::{Compression, write::GzEncoder};
use serde::{Deserialize, Serialize};

use crate::{
    folder::{self, CLIENT_LOG_FILE, CORE_CRASH_FILE},
    redact::{Learned, Redactor},
    report::{Kind, Report},
    section::MAX_SECTION_BYTES,
    stack::{self, Stack},
};

pub use signature::signature;

/// Version of the payload; the server rejects others.
pub const SCHEMA: u32 = 1;
/// Largest gzip body sent; the server rejects larger ones.
pub const MAX_COMPRESSED_BYTES: usize = 64 << 10;
/// Largest JSON before compression; the server stops inflating past it.
pub const MAX_JSON_BYTES: usize = 512 << 10;
/// Newest client log lines sent.
pub const MAX_LOG_LINES: usize = 200;
/// Characters kept of one log line.
const MAX_LINE_CHARS: usize = 1_000;
/// Characters kept of the message, backtrace and span trace.
const MAX_TEXT_CHARS: usize = 16 << 10;
/// Length of install and report ids: 128 random bits in lowercase hex.
pub const ID_LEN: usize = 32;
/// What replaces a log line that cannot be redacted safely.
const UNREDACTABLE_LINE: &str = "[line removed: escaped too deeply to redact]";

/// One crash as the server stores it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Payload {
    pub schema: u32,
    /// Random per report, so a resent report is stored once.
    pub report_id: String,
    /// Random per install; never derived from an account or device.
    pub install_id: String,
    pub signature: String,
    pub kind: Kind,
    pub release: String,
    pub commit: String,
    pub build_profile: String,
    pub os: String,
    pub arch: String,
    pub gpu: Option<String>,
    pub driver: Option<String>,
    /// The renderer backend, such as `vulkan`.
    pub backend: Option<String>,
    pub uptime_ms: u64,
    /// `report.json` without its session id, user comment or native details, redacted.
    pub report: serde_json::Value,
    /// Native fault details: the reason, fault address and faulting module offset.
    pub native: Option<serde_json::Value>,
    pub stack: Option<Stack>,
    /// The client log's newest lines, redacted, oldest first.
    pub log: Vec<String>,
}

/// The two random ids a payload names.
#[derive(Clone, Copy, Debug)]
pub struct Ids<'a> {
    pub install: &'a str,
    pub report: &'a str,
}

/// True for an id of [`ID_LEN`] lowercase hex digits.
pub fn valid_id(id: &str) -> bool {
    id.len() == ID_LEN
        && id
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

/// Builds the payload for the crash folder at `folder`; `home` is the player's home directory,
/// learned as private wherever it appears.
pub fn build(folder: &Path, ids: Ids<'_>, home: Option<&str>) -> io::Result<Payload> {
    if !valid_id(ids.install) || !valid_id(ids.report) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "malformed upload id",
        ));
    }
    let report = folder::read(folder)?;
    let log = read_bounded(&folder.join(CLIENT_LOG_FILE));
    let core_crash = read_bounded(&folder.join(CORE_CRASH_FILE));
    let stack = stack::read(folder);
    payload(&report, &log, &core_crash, stack, ids, home)
}

/// [`build`] from already-read folder members. Any member that cannot be fully learned returns
/// `InvalidData`, so unidentified private values cannot leak through another field.
pub fn payload(
    report: &Report,
    log: &str,
    core_crash: &str,
    stack: Option<Stack>,
    ids: Ids<'_>,
    home: Option<&str>,
) -> io::Result<Payload> {
    let mut value = serde_json::to_value(report).unwrap_or_default();
    let mut learned = Learned::default();
    if let Some(home) = home.map(|home| home.trim_end_matches(['/', '\\']))
        && !home.is_empty()
    {
        learned.value(home);
    }
    learned.json(&value);
    learn_value(&mut learned, &value)?;
    // The process session id is random, but it ties a report to the player's local logs.
    if !report.session_id.is_empty() {
        learned.value(report.session_id.as_str());
    }
    for text in [log, core_crash] {
        learn_text(&mut learned, text)?;
    }
    if let Some(stack) = &stack {
        let value = serde_json::to_value(stack)?;
        learned.json(&value);
        learn_value(&mut learned, &value)?;
    }
    if let Some(fields) = value.as_object_mut() {
        for private in ["session_id", "user_comment", "native"] {
            fields.remove(private);
        }
    }
    let redactor = learned.redactor();
    if redact_report(&mut value, &redactor).is_err() {
        value = serde_json::json!({ "message": "[report removed: escaped too deeply to redact]" });
    }
    bound_strings(&mut value);
    let text = |text: Option<&str>| text.and_then(|text| redactor.text(text).ok());
    let log = log_tail(&redactor, log);
    let native = report
        .native
        .as_ref()
        .map(|native| native_fields(native, &redactor));
    let redacted_message = value
        .get("message")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let core_line = text(Some(core_crash)).unwrap_or_default();
    let stack = stack.map(|stack| {
        let mut stack = stack.sanitized();
        stack.reason = redactor.text(&stack.reason).unwrap_or_default();
        for module in &mut stack.modules {
            module.name = redactor.file_name(&module.name).unwrap_or_default();
        }
        for frame in &mut stack.frames {
            frame.function = frame
                .function
                .as_deref()
                .and_then(|name| redactor.text(name).ok());
        }
        stack.sanitized()
    });
    let stack = stack.or_else(|| {
        native
            .as_ref()
            .and_then(|native| native.get("reason"))
            .and_then(serde_json::Value::as_str)
            .map(|reason| Stack {
                reason: reason.to_owned(),
                ..Stack::default()
            })
    });
    Ok(Payload {
        schema: SCHEMA,
        report_id: ids.report.to_owned(),
        install_id: ids.install.to_owned(),
        signature: signature(report.kind, &redacted_message, &core_line, stack.as_ref()),
        kind: report.kind,
        release: text(Some(&report.release))
            .map(|s| bounded_line(&s))
            .unwrap_or_default(),
        commit: text(Some(&report.commit))
            .map(|s| bounded_line(&s))
            .unwrap_or_default(),
        build_profile: text(Some(&report.build_profile))
            .map(|s| bounded_line(&s))
            .unwrap_or_default(),
        os: text(Some(&report.os))
            .map(|s| bounded_line(&s))
            .unwrap_or_default(),
        arch: text(Some(&report.arch))
            .map(|s| bounded_line(&s))
            .unwrap_or_default(),
        gpu: text(report.gpu.as_deref()).map(|gpu| bounded_line(&gpu)),
        driver: text(report.driver.as_deref()).map(|driver| bounded_line(&driver)),
        backend: report
            .renderer
            .as_ref()
            .and_then(|renderer| renderer.get("backend"))
            .and_then(serde_json::Value::as_str)
            .and_then(|backend| redactor.text(backend).ok())
            .map(|backend| {
                backend
                    .chars()
                    .filter(char::is_ascii_alphanumeric)
                    .take(16)
                    .collect()
            }),
        uptime_ms: report.uptime_ms,
        report: value,
        native,
        stack,
        log,
    })
}

/// `payload` as gzip JSON of at most [`MAX_COMPRESSED_BYTES`], dropping the oldest log lines,
/// then the backtrace, then modules no frame names, until it fits.
pub fn encode(mut payload: Payload) -> io::Result<Vec<u8>> {
    let mut backtrace_dropped = false;
    loop {
        let json = serde_json::to_vec(&payload)?;
        if json.len() <= MAX_JSON_BYTES {
            let body = gzip(&json)?;
            if body.len() <= MAX_COMPRESSED_BYTES {
                return Ok(body);
            }
        }
        if !payload.log.is_empty() {
            let drop = payload.log.len().div_ceil(4);
            payload.log.drain(..drop);
        } else if !backtrace_dropped {
            backtrace_dropped = true;
            if let Some(fields) = payload.report.as_object_mut() {
                fields.insert(
                    "backtrace".into(),
                    "[removed to fit the upload limit]".into(),
                );
                fields.remove("span_trace");
            }
        } else if let Some(stack) = payload.stack.as_mut().filter(|stack| {
            stack.modules.len()
                > stack
                    .frames
                    .iter()
                    .filter(|frame| frame.module.is_some())
                    .count()
        }) {
            let used: Vec<bool> = (0..stack.modules.len())
                .map(|index| stack.frames.iter().any(|frame| frame.module == Some(index)))
                .collect();
            let mut renumber = Vec::with_capacity(used.len());
            let mut next = 0;
            for used in &used {
                renumber.push(next);
                next += usize::from(*used);
            }
            let mut index = 0;
            stack.modules.retain(|_| {
                index += 1;
                used[index - 1]
            });
            for frame in &mut stack.frames {
                frame.module = frame.module.map(|module| renumber[module]);
            }
        } else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "crash payload exceeds the upload limit",
            ));
        }
    }
}

/// Compresses bounded JSON for the HTTP body.
fn gzip(bytes: &[u8]) -> io::Result<Vec<u8>> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(bytes)?;
    encoder.finish()
}

/// The newest [`MAX_LOG_LINES`] lines of `log`, each redacted and bounded.
fn log_tail(redactor: &Redactor, log: &str) -> Vec<String> {
    let lines: Vec<&str> = log.lines().collect();
    lines[lines.len().saturating_sub(MAX_LOG_LINES)..]
        .iter()
        .map(|line| {
            redactor
                .text(line)
                .map(|line| line.chars().take(MAX_LINE_CHARS).collect())
                .unwrap_or_else(|_| UNREDACTABLE_LINE.to_owned())
        })
        .collect()
}

/// Learns every JSON string before any member is dropped, so private values cannot cross fields.
fn learn_value(learned: &mut Learned, value: &serde_json::Value) -> io::Result<()> {
    match value {
        serde_json::Value::String(text) => learn_text(learned, text)?,
        serde_json::Value::Array(values) => {
            for value in values {
                learn_value(learned, value)?;
            }
        }
        serde_json::Value::Object(fields) => {
            for (key, value) in fields {
                learn_text(learned, key)?;
                learn_value(learned, value)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Rejects a payload if any member cannot contribute its private values to the shared redactor.
fn learn_text(learned: &mut Learned, text: &str) -> io::Result<()> {
    learned.text(text).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "crash data cannot be safely redacted",
        )
    })
}

/// Preserves the typed report's field names and kind while scrubbing their free-text values.
/// Renderer and game-state maps keep full key redaction because their keys can contain user data.
fn redact_report(
    value: &mut serde_json::Value,
    redactor: &Redactor,
) -> Result<(), crate::redact::Error> {
    if let Some(fields) = value.as_object_mut() {
        for (key, value) in fields {
            if key == "kind" {
                continue;
            }
            if key == "thread"
                && let Some(thread) = value.as_object_mut()
            {
                for value in thread.values_mut() {
                    redactor.json(value)?;
                    if let serde_json::Value::String(text) = value {
                        *text = bounded_line(text);
                    }
                }
            } else {
                redactor.json(value)?;
                if !matches!(key.as_str(), "message" | "backtrace" | "span_trace")
                    && let serde_json::Value::String(text) = value
                {
                    *text = bounded_line(text);
                }
            }
        }
    }
    Ok(())
}

/// The native fields a server may index: the reason as redacted text, addresses as hex and
/// the faulting module's file name.
fn native_fields(native: &serde_json::Value, redactor: &Redactor) -> serde_json::Value {
    let hex = |value: Option<&serde_json::Value>| {
        value
            .and_then(serde_json::Value::as_str)
            .filter(|text| text.starts_with("0x") && (3..=18).contains(&text.len()))
            .filter(|text| text[2..].bytes().all(|byte| byte.is_ascii_hexdigit()))
            .map(str::to_owned)
    };
    let mut out = serde_json::Map::new();
    if let Some(reason) = native.get("reason").and_then(serde_json::Value::as_str)
        && let Ok(reason) = redactor.text(reason)
    {
        out.insert("reason".into(), bounded_line(&reason).into());
    }
    if !out.contains_key("reason")
        && let Some(code) = hex(native.get("exception_code"))
    {
        out.insert("reason".into(), format!("Windows exception {code}").into());
    }
    if !out.contains_key("reason")
        && let Some(signal) = native.get("signal").and_then(serde_json::Value::as_i64)
    {
        let code = native
            .get("signal_code")
            .and_then(serde_json::Value::as_i64);
        let reason = code.map_or_else(
            || format!("signal {signal}"),
            |code| format!("signal {signal} / code {code}"),
        );
        out.insert("reason".into(), reason.into());
    }
    if let Some(address) = native
        .get("fault_address")
        .and_then(serde_json::Value::as_u64)
    {
        out.insert("address".into(), format!("0x{address:X}").into());
    }
    for key in ["address", "exit_code"] {
        if let Some(value) = hex(native.get(key)) {
            out.insert(key.into(), value.into());
        }
    }
    if let Some(fault) = native.get("fault") {
        let mut location = serde_json::Map::new();
        for key in ["address", "offset"] {
            if let Some(value) = hex(fault.get(key)) {
                location.insert(key.into(), value.into());
            }
        }
        if let Some(module) = fault.get("module").and_then(serde_json::Value::as_str) {
            location.insert(
                "module".into(),
                bounded_line(
                    &redactor
                        .file_name(crate::native::module_name(module))
                        .unwrap_or_default(),
                )
                .into(),
            );
        }
        out.insert("fault".into(), location.into());
    }
    out.into()
}

/// Truncates every string in `value` to [`MAX_TEXT_CHARS`].
fn bound_strings(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::String(text) if text.chars().count() > MAX_TEXT_CHARS => {
            *text = text.chars().take(MAX_TEXT_CHARS).collect();
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(bound_strings),
        serde_json::Value::Object(items) => items.values_mut().for_each(bound_strings),
        _ => {}
    }
}

/// The first line of `text`, at most 200 characters.
fn bounded_line(text: &str) -> String {
    text.lines()
        .next()
        .unwrap_or_default()
        .chars()
        .take(200)
        .collect()
}

/// A folder member as text, up to a log section's size; missing or unreadable is empty.
fn read_bounded(path: &Path) -> String {
    let mut bytes = Vec::new();
    if let Ok(file) = fs::File::open(path) {
        let _ = file.take(MAX_SECTION_BYTES).read_to_end(&mut bytes);
    }
    String::from_utf8_lossy(&bytes).into_owned()
}
