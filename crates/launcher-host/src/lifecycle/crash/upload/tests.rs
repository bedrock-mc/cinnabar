use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use crash_report::{
    folder::{self, Logs},
    report::{Identity, Kind, Report},
    upload::Payload,
};

use super::*;

/// A loopback endpoint that answers every request with `status` and keeps each body.
struct Server {
    url: url::Url,
    bodies: Arc<Mutex<Vec<Vec<u8>>>>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Server {
    /// Starts a bounded-lifetime fixture endpoint with a fixed response status.
    fn start(status: u16) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = endpoint(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        let bodies = Arc::new(Mutex::new(Vec::new()));
        let kept = Arc::clone(&bodies);
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = Arc::clone(&stop);
        let worker = std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                if stopping.load(Ordering::Acquire) {
                    break;
                }
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" || line.is_empty() {
                        break;
                    }
                    if let Some((name, value)) = line.split_once(':')
                        && name.eq_ignore_ascii_case("content-length")
                    {
                        length = value.trim().parse().unwrap();
                    }
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                kept.lock().unwrap().push(body);
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} X\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
            }
        });
        Self {
            url,
            bodies,
            stop,
            worker: Some(worker),
        }
    }

    /// Decodes every completed request for behavioral assertions.
    fn payloads(&self) -> Vec<Payload> {
        self.bodies
            .lock()
            .unwrap()
            .iter()
            .map(|body| {
                let mut json = Vec::new();
                flate2::read::GzDecoder::new(body.as_slice())
                    .read_to_end(&mut json)
                    .unwrap();
                serde_json::from_slice(&json).unwrap()
            })
            .collect()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = std::net::TcpStream::connect(("127.0.0.1", self.url.port().unwrap()));
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap();
        }
    }
}

/// Isolates all saved data and settings from the real install.
fn layout(root: &Path) -> InstallLayout {
    let mut layout = launcher::test_support::checkout();
    layout.user_data_root = root.join("data");
    layout.user_config_root = root.join("config");
    layout
}

/// Writes one synthetic crash folder.
fn crash(layout: &InstallLayout, message: &str) -> PathBuf {
    let identity = Identity {
        session_id: "0123456789abcdef".into(),
        started_at_ms: 0,
        release: "0.1.26".into(),
        commit: "abc".into(),
        build_profile: "release".into(),
    };
    let report = Report::new(Kind::Panic, message.into(), &identity, now_ms());
    folder::write(
        &layout.crash_dir(),
        &report,
        &Logs::default(),
        None,
        now_ms(),
    )
    .unwrap()
}

/// Selects explicit consent behavior against the local fixture.
fn policy(server: &Server, automatic: bool) -> Policy {
    Policy {
        endpoint: server.url.clone(),
        automatic,
    }
}

/// Persists the player's automatic-upload preference independently of a cached worker policy.
fn set_automatic(layout: &InstallLayout, enabled: bool) {
    use launcher::menu::settings_options::SettingId;
    let mut settings = SettingsOptions::default();
    settings.set(SettingId::SendCrashReports.index(), i32::from(enabled));
    settings
        .save(&layout.user_config_root.join(SETTINGS_FILE))
        .unwrap();
}

#[test]
fn completed_fallback_reports_are_recovered_without_enrolling_in_progress_reports() {
    let root = tempfile::tempdir().unwrap();
    let layout = layout(root.path());
    let identity = Identity {
        session_id: "fallback".into(),
        started_at_ms: 0,
        release: "fixture".into(),
        commit: "fixture".into(),
        build_profile: "test".into(),
    };
    let context = super::super::context(&layout, identity);
    let ready = super::super::fallback::write_panic(
        &context,
        "fallback panic".into(),
        Default::default(),
        String::new(),
        now_ms(),
    )
    .unwrap();
    let unfinished = crash(&layout, "another producer has not finished its attachments");
    set_automatic(&layout, true);
    let server = Server::start(201);
    let results = send_due(&layout, &policy(&server, true));
    assert_eq!(results.len(), 1);
    assert!(matches!(state::read(&ready), Some(Upload::Sent { .. })));
    assert!(state::read(&unfinished).is_none());
    assert_eq!(server.payloads().len(), 1);
    assert!(send_due(&layout, &policy(&server, true)).is_empty());
}

#[test]
fn android_saved_reports_send_on_a_healthy_launch_without_exit_info() {
    let root = tempfile::tempdir().unwrap();
    let layout = layout(root.path());
    let folder = crash(&layout, "panicked at a.rs:1:1:\nlegacy API crash");
    let log = layout.log_dir().join("client.log");
    std::fs::create_dir_all(layout.log_dir()).unwrap();
    std::fs::write(
        &log,
        "CLIENT_SESSION_START session=0123456789abcdef\nprevious-session-log\n",
    )
    .unwrap();
    let snapshot = previous_exit::Snapshot::capture(&layout.crash_dir(), &log);
    assert_eq!(snapshot.latest_saved(), Some(folder.as_path()));
    std::fs::write(&log, "CLIENT_SESSION_START session=new\nnew-session-log\n").unwrap();
    set_automatic(&layout, true);
    for saved in snapshot.complete_saved().unwrap() {
        completed(&layout, &saved);
    }
    let server = Server::start(201);
    assert_eq!(send_due(&layout, &policy(&server, true)).len(), 1);
    let payloads = server.payloads();
    assert_eq!(payloads.len(), 1);
    assert!(
        payloads[0]
            .log
            .iter()
            .any(|line| line.contains("previous-session-log"))
    );
    assert!(
        !payloads[0]
            .log
            .iter()
            .any(|line| line.contains("new-session-log"))
    );
    assert!(matches!(state::read(&folder), Some(Upload::Sent { .. })));
    assert!(send_due(&layout, &policy(&server, true)).is_empty());
}

#[test]
fn nothing_is_sent_without_consent() {
    let root = tempfile::tempdir().unwrap();
    let layout = layout(root.path());
    let server = Server::start(201);
    let folder = crash(&layout, "panicked at a.rs:1:1:\nboom");
    assert!(send_due(&layout, &policy(&server, false)).is_empty());
    assert!(
        send_due(&layout, &policy(&server, true)).is_empty(),
        "producers must complete a report before granting automatic consent"
    );
    assert!(send(&layout, &folder, &policy(&server, false)).is_err());
    assert!(server.payloads().is_empty());
    assert!(state::read(&folder).is_none());
}

#[test]
fn automatic_consent_sends_each_report_once() {
    let root = tempfile::tempdir().unwrap();
    let layout = layout(root.path());
    set_automatic(&layout, true);
    let server = Server::start(201);
    let first = crash(&layout, "panicked at a.rs:1:1:\nboom");
    consent(&first, Consent::Auto, now_ms()).unwrap();
    let results = send_due(&layout, &policy(&server, true));
    assert_eq!(results.len(), 1);
    assert!(matches!(results[0].1, Ok(Upload::Sent { .. })));
    assert!(send_due(&layout, &policy(&server, true)).is_empty());
    assert!(matches!(
        send(&layout, &first, &policy(&server, true)),
        Ok(Upload::Sent { .. })
    ));
    let payloads = server.payloads();
    assert_eq!(payloads.len(), 1, "a sent report is never sent again");
    let install = install_id(&layout.user_data_root).unwrap();
    assert_eq!(payloads[0].install_id, install);
    assert_eq!(payloads[0].signature, "panic: a.rs:1: boom");
}

#[test]
fn a_players_send_report_sends_that_folder_only() {
    let root = tempfile::tempdir().unwrap();
    let layout = layout(root.path());
    let server = Server::start(201);
    let other = crash(&layout, "other");
    let chosen = crash(&layout, "chosen");
    consent(&chosen, Consent::User, now_ms()).unwrap();
    assert!(matches!(
        send(&layout, &chosen, &policy(&server, false)),
        Ok(Upload::Sent { .. })
    ));
    assert!(send_due(&layout, &policy(&server, false)).is_empty());
    assert_eq!(server.payloads().len(), 1);
    assert!(state::read(&other).is_none());
}

#[test]
fn failed_sends_stay_pending_and_refusals_are_abandoned() {
    let root = tempfile::tempdir().unwrap();
    let layout = layout(root.path());
    let unavailable = Server::start(503);
    let folder = crash(&layout, "boom");
    consent(&folder, Consent::User, now_ms()).unwrap();
    let pending = send(&layout, &folder, &policy(&unavailable, false)).unwrap();
    assert!(matches!(pending, Upload::Pending { attempts: 1, .. }));
    assert!(
        send_due(&layout, &policy(&unavailable, false)).is_empty(),
        "the retry waits for its backoff"
    );
    let refused = Server::start(400);
    assert!(matches!(
        send(&layout, &folder, &policy(&refused, false)).unwrap(),
        Upload::Abandoned { .. }
    ));
    let unreachable = Policy {
        endpoint: endpoint("http://127.0.0.1:9").unwrap(),
        automatic: true,
    };
    let other = crash(&layout, "offline");
    set_automatic(&layout, true);
    consent(&other, Consent::Auto, now_ms()).unwrap();
    let results = send_due(&layout, &unreachable);
    assert_eq!(results.len(), 1);
    assert!(matches!(results[0].1, Ok(Upload::Pending { .. })));
    assert!(matches!(state::read(&other), Some(Upload::Pending { .. })));
}

#[test]
fn the_install_id_is_random_and_stable() {
    let root = tempfile::tempdir().unwrap();
    let id = install_id(root.path()).unwrap();
    assert!(crash_report::upload::valid_id(&id));
    assert_eq!(install_id(root.path()).unwrap(), id);
    let other = tempfile::tempdir().unwrap();
    assert_ne!(install_id(other.path()).unwrap(), id);
}

#[test]
fn plain_http_reaches_only_loopback_hosts() {
    assert!(endpoint("https://api.cinnabar.gg").is_some());
    assert_eq!(
        endpoint("https://api.cinnabar.gg").unwrap().as_str(),
        "https://api.cinnabar.gg/v1/crash-reports"
    );
    assert!(endpoint("http://127.0.0.1:8787").is_some());
    assert!(endpoint("http://localhost:8787").is_some());
    assert!(endpoint("http://example.com").is_none());
    assert!(endpoint("ftp://127.0.0.1").is_none());
}

#[test]
fn statuses_map_to_sent_retry_or_refused() {
    assert_eq!(classify(201, None), Outcome::Sent);
    assert!(matches!(classify(409, None), Outcome::Rejected(_)));
    assert!(matches!(classify(413, None), Outcome::Rejected(_)));
    assert_eq!(
        classify(429, Some(30_000)),
        Outcome::Retry {
            after_ms: Some(30_000)
        }
    );
    assert_eq!(classify(404, None), Outcome::Retry { after_ms: None });
}

#[test]
fn turning_automatic_uploads_off_also_blocks_pending_auto_reports() {
    let root = tempfile::tempdir().unwrap();
    let layout = layout(root.path());
    let server = Server::start(201);
    let folder = crash(&layout, "boom");
    consent(&folder, Consent::Auto, now_ms()).unwrap();
    assert!(send(&layout, &folder, &policy(&server, false)).is_err());
    assert!(server.payloads().is_empty());
    consent(&folder, Consent::User, now_ms()).unwrap();
    assert!(matches!(
        send(&layout, &folder, &policy(&server, false)),
        Ok(Upload::Sent { .. })
    ));
    assert_eq!(server.payloads().len(), 1);
}

#[test]
fn concurrent_senders_store_one_report() {
    let root = tempfile::tempdir().unwrap();
    let layout = layout(root.path());
    let server = Server::start(201);
    let folder = crash(&layout, "boom");
    consent(&folder, Consent::User, now_ms()).unwrap();
    let policy = policy(&server, false);
    std::thread::scope(|scope| {
        let a = scope.spawn(|| send(&layout, &folder, &policy));
        let b = scope.spawn(|| send(&layout, &folder, &policy));
        let _ = a.join().unwrap();
        let _ = b.join().unwrap();
    });
    assert_eq!(server.payloads().len(), 1);
    assert!(matches!(state::read(&folder), Some(Upload::Sent { .. })));
}

/// Used only by the kill-switch test's isolated child process.
const KILL_SWITCH_FIXTURE_ROOT: &str = "CINNABAR_CRASH_KILL_SWITCH_FIXTURE";

#[test]
fn kill_switch_fixture() {
    let Ok(root) = std::env::var(KILL_SWITCH_FIXTURE_ROOT) else {
        return;
    };
    let layout = layout(Path::new(&root));
    assert!(disabled());
    assert!(Policy::load(&layout).is_none());
    let policy = Policy {
        endpoint: endpoint(&std::env::var(URL_ENV).unwrap()).unwrap(),
        automatic: true,
    };
    let folder = crash(&layout, "boom");
    consent(&folder, Consent::User, now_ms()).unwrap();
    assert_eq!(
        send(&layout, &folder, &policy).unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
    assert!(send_due(&layout, &policy).is_empty());
}

#[test]
fn the_kill_switch_overrides_explicit_and_automatic_consent() {
    let root = tempfile::tempdir().unwrap();
    let server = Server::start(201);
    let test = format!(
        "{}::kill_switch_fixture",
        module_path!().split_once("::").unwrap().1
    );
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", &test])
        .env(KILL_SWITCH_FIXTURE_ROOT, root.path())
        .env(DISABLE_ENV, "0")
        .env(URL_ENV, server.url.as_str())
        .status()
        .unwrap();
    assert!(status.success());
    assert!(server.payloads().is_empty());
}

#[test]
fn automatic_consent_is_recorded_only_when_the_producer_completes_the_report() {
    use launcher::menu::settings_options::SettingId;
    let root = tempfile::tempdir().unwrap();
    let layout = layout(root.path());
    let server = Server::start(201);
    let folder = crash(&layout, "boom");
    let mut settings = SettingsOptions::default();
    settings.set(SettingId::SendCrashReports.index(), 1);
    settings
        .save(&layout.user_config_root.join(SETTINGS_FILE))
        .unwrap();
    assert!(send_due(&layout, &policy(&server, true)).is_empty());
    completed(&layout, &folder);
    assert!(matches!(
        state::read(&folder),
        Some(Upload::Pending {
            consent: Consent::Auto,
            ..
        })
    ));
    let results = send_due(&layout, &policy(&server, true));
    assert!(matches!(results[0].1, Ok(Upload::Sent { .. })));
    assert_eq!(server.payloads().len(), 1);
}

#[test]
fn a_stale_scheduled_worker_respects_a_new_retry_deadline() {
    let root = tempfile::tempdir().unwrap();
    let layout = layout(root.path());
    let folder = crash(&layout, "boom");
    let server = Server::start(201);
    consent(&folder, Consent::User, now_ms()).unwrap();
    assert_eq!(
        state::due(&layout.crash_dir(), now_ms()),
        vec![folder.clone()]
    );
    // Another worker persisted a server-requested delay after this worker collected its queue.
    let delayed = state::record(
        &folder,
        &Outcome::Retry {
            after_ms: Some(3_600_000),
        },
        now_ms(),
    )
    .unwrap();
    assert_eq!(
        send_inner(&layout, &folder, &policy(&server, false), true).unwrap(),
        delayed
    );
    assert!(server.payloads().is_empty());
}

#[test]
fn a_cached_policy_cannot_send_after_automatic_consent_is_withdrawn() {
    let root = tempfile::tempdir().unwrap();
    let layout = layout(root.path());
    let server = Server::start(201);
    let folder = crash(&layout, "boom");
    set_automatic(&layout, true);
    consent(&folder, Consent::Auto, now_ms()).unwrap();
    let cached = policy(&server, true);
    set_automatic(&layout, false);
    assert!(send(&layout, &folder, &cached).is_err());
    assert!(server.payloads().is_empty());
    assert!(matches!(
        state::read(&folder),
        Some(Upload::Pending { attempts: 0, .. })
    ));
}
