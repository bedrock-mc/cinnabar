//! `report.json`, schema version 1.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Version written to every report; readers reject other versions.
pub const SCHEMA: u32 = 1;

/// What ended or stalled the process.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A Rust panic.
    Panic,
    /// A native fault: an unhandled exception or a fatal signal.
    Native,
    /// The process exited without a dump.
    Abort,
    /// The main thread stopped making progress.
    Hang,
    /// The Go core process exited abnormally.
    Core,
    /// The client failed from `main` before the game started, such as on a missing carrier.
    Startup,
    /// The client stopped from `main` on a fatal error after the game started, such as a network
    /// session that ended unexpectedly.
    Runtime,
}

/// The reporting thread; `id` is an opaque platform- or runtime-defined identifier.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Thread {
    pub name: Option<String>,
    pub id: String,
}

impl Thread {
    /// Names the calling thread by its Rust name and `ThreadId`.
    pub fn current() -> Self {
        let thread = std::thread::current();
        Self {
            name: thread.name().map(str::to_owned),
            id: format!("{:?}", thread.id()),
        }
    }
}

/// Build and session identity of the reporting process, fixed for its lifetime.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    /// Process session id, also printed in the session's log header.
    pub session_id: String,
    /// Unix milliseconds when the session began; uptime counts from here.
    pub started_at_ms: u64,
    pub release: String,
    pub commit: String,
    /// `debug` or `release`.
    pub build_profile: String,
}

/// One crash. Optional context stays `None` or empty when the reporter does not know it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Report {
    pub schema: u32,
    pub kind: Kind,
    pub message: String,
    pub thread: Thread,
    /// Rendered backtrace of the reporting thread; empty when none was captured.
    pub backtrace: String,
    pub release: String,
    pub commit: String,
    pub build_profile: String,
    pub os: String,
    pub arch: String,
    pub gpu: Option<String>,
    pub driver: Option<String>,
    pub uptime_ms: u64,
    pub session_id: String,
    /// The renderer startup attempt of this process, as its owner serializes it.
    pub renderer: Option<serde_json::Value>,
    /// Caller-published game state such as session phase or dimension.
    pub game_state: BTreeMap<String, String>,
    pub span_trace: Option<String>,
    /// Text the user added after the crash.
    pub user_comment: Option<String>,
    /// Platform fault details for `native` reports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native: Option<serde_json::Value>,
}

impl Report {
    /// Starts a report at `now_ms` (Unix milliseconds) for the process described by `identity`.
    pub fn new(kind: Kind, message: String, identity: &Identity, now_ms: u64) -> Self {
        Self {
            schema: SCHEMA,
            kind,
            message,
            thread: Thread::default(),
            backtrace: String::new(),
            release: identity.release.clone(),
            commit: identity.commit.clone(),
            build_profile: identity.build_profile.clone(),
            os: std::env::consts::OS.to_owned(),
            arch: std::env::consts::ARCH.to_owned(),
            gpu: None,
            driver: None,
            uptime_ms: now_ms.saturating_sub(identity.started_at_ms),
            session_id: identity.session_id.clone(),
            renderer: None,
            game_state: BTreeMap::new(),
            span_trace: None,
            user_comment: None,
            native: None,
        }
    }

    /// Serializes every field except `deferred`, leaving the JSON object open.
    ///
    /// A writer that cannot allocate, such as a signal handler, appends each deferred field as
    /// `,"name":value` and then the closing `}`.
    pub fn open_json(&self, deferred: &[&str]) -> serde_json::Result<Vec<u8>> {
        let mut value = serde_json::to_value(self)?;
        if let Some(fields) = value.as_object_mut() {
            for field in deferred {
                fields.remove(*field);
            }
        }
        let mut bytes = serde_json::to_vec(&value)?;
        bytes.pop();
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Supplies a stable session identity for report serialization fixtures.
    fn identity() -> Identity {
        Identity {
            session_id: "0123456789abcdef".into(),
            started_at_ms: 1_000,
            release: "1.2.3".into(),
            commit: "abc123".into(),
            build_profile: "release".into(),
        }
    }

    /// Fills every optional context field for report round-trip tests.
    fn complete() -> Report {
        let mut report = Report::new(Kind::Native, "fault".into(), &identity(), 4_500);
        report.thread = Thread {
            name: Some("main".into()),
            id: "7".into(),
        };
        report.backtrace = "0: frame".into();
        report.gpu = Some("GPU".into());
        report.driver = Some("driver 1".into());
        report.renderer = Some(serde_json::json!({"backend": "vulkan", "stage": "ready"}));
        report
            .game_state
            .insert("dimension".into(), "overworld".into());
        report.span_trace = Some("in join".into());
        report.user_comment = Some("clicked play".into());
        report.native = Some(serde_json::json!({"signal": 11}));
        report
    }

    #[test]
    fn schema_round_trips_every_field() {
        let report = complete();
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["schema"], 1);
        assert_eq!(json["kind"], "native");
        assert_eq!(json["uptime_ms"], 3_500);
        assert_eq!(json["thread"]["name"], "main");
        assert_eq!(json["os"], std::env::consts::OS);
        let decoded: Report = serde_json::from_value(json).unwrap();
        assert_eq!(decoded, report);
        let mut minimal = Report::new(Kind::Panic, "boom".into(), &identity(), 0);
        minimal.thread = Thread::current();
        let decoded: Report =
            serde_json::from_slice(&serde_json::to_vec(&minimal).unwrap()).unwrap();
        assert_eq!(decoded, minimal);
        assert_eq!(decoded.uptime_ms, 0);
        assert!(
            serde_json::to_value(&minimal)
                .unwrap()
                .get("native")
                .is_none()
        );
    }

    #[test]
    fn every_kind_has_its_documented_name() {
        for (kind, name) in [
            (Kind::Panic, "panic"),
            (Kind::Native, "native"),
            (Kind::Abort, "abort"),
            (Kind::Hang, "hang"),
            (Kind::Core, "core"),
            (Kind::Startup, "startup"),
            (Kind::Runtime, "runtime"),
        ] {
            assert_eq!(serde_json::to_value(kind).unwrap(), name);
        }
    }

    #[test]
    fn open_json_completes_to_the_same_report() {
        let report = complete();
        let mut bytes = report
            .open_json(&["uptime_ms", "thread", "native"])
            .unwrap();
        bytes.extend_from_slice(
            br#","uptime_ms":3500,"thread":{"name":"main","id":"7"},"native":{"signal":11}}"#,
        );
        let decoded: Report = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded, report);
    }
}
