use super::*;
use server_experience::manifest::Scope;
use std::collections::BTreeSet;

/// Builds startup metadata without enabling developer execution in the test environment.
fn startup() -> Start {
    Start {
        protocol: HELPER_PROTOCOL,
        owner: Principal {
            session: "session".into(),
            bundle: "fixture".into(),
            generation: INITIAL_BUNDLE_GENERATION,
        },
        capabilities: Capabilities {
            scope: Scope {
                permissions: BTreeSet::new(),
                origins: BTreeSet::new(),
                memory_bytes: 0,
                gpu_bytes: 0,
            },
            assets: BTreeSet::new(),
            templates: BTreeSet::new(),
            channels: Vec::new(),
            actions: BTreeSet::new(),
            max_message_bytes: MAX_MESSAGE_BYTES as u32,
        },
        epoch: 1,
        component: String::new(),
    }
}

/// Holds component preparation until the test allows the supervisor to attempt launch.
fn delayed_helper(executable: &Path) -> (Helper, mpsc::Sender<()>) {
    let (release, gate) = mpsc::channel();
    let (entered, started) = mpsc::channel();
    let since = Instant::now();
    let helper = Helper::spawn_pending(executable, move || {
        entered.send(())?;
        gate.recv_timeout(Duration::from_secs(5))?;
        Ok(startup())
    })
    .unwrap();
    assert!(since.elapsed() < Duration::from_secs(1));
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    (helper, release)
}

/// Polls with a bounded test wait so asynchronous launch errors cannot hang the suite.
fn completion(helper: &mut Helper) -> Result<Reply> {
    let since = Instant::now();
    loop {
        if let Some(result) = helper.poll() {
            return result;
        }
        assert!(since.elapsed() < Duration::from_secs(5));
        test_time::idle();
    }
}

#[test]
fn delayed_start_keeps_frame_polling_responsive_and_reports_launch_failure() {
    let directory = tempfile::tempdir().unwrap();
    let (mut helper, release) = delayed_helper(&directory.path().join("missing-helper"));
    let since = Instant::now();
    for _ in 0..100 {
        assert!(helper.poll().is_none());
    }
    assert!(since.elapsed() < Duration::from_secs(1));
    assert!(
        helper
            .dispatch(Dispatch {
                event: Event::Epoch,
                epoch: 1,
                gui: None,
            })
            .is_err()
    );
    release.send(()).unwrap();
    let error = completion(&mut helper).unwrap_err();
    assert_eq!(
        error.downcast_ref::<std::io::Error>().unwrap().kind(),
        std::io::ErrorKind::NotFound
    );
    assert!(helper.quarantined);
    assert!(helper.poll().is_none());
}

#[test]
fn startup_deadline_revokes_launch_before_preparation_completes() {
    let directory = tempfile::tempdir().unwrap();
    let (mut helper, release) = delayed_helper(&directory.path().join("missing-helper"));
    helper.pending_since = Some(Instant::now() - HELPER_DEADLINE);
    assert_eq!(
        helper.poll().unwrap().unwrap_err().to_string(),
        "helper deadline exceeded"
    );
    release.send(()).unwrap();
    let error = helper
        .responses
        .lock()
        .unwrap()
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap_err();
    assert_eq!(error.to_string(), "helper startup revoked");
    assert!(helper.poll().is_none());
}

#[test]
fn dropping_a_pending_helper_revokes_its_launch() {
    let directory = tempfile::tempdir().unwrap();
    let (helper, release) = delayed_helper(&directory.path().join("missing-helper"));
    // Keep the result receiver so the supervisor's cancellation can be observed after drop.
    let (_, replacement) = mpsc::sync_channel(1);
    let responses = std::mem::replace(&mut *helper.responses.lock().unwrap(), replacement);
    let since = Instant::now();
    drop(helper);
    assert!(since.elapsed() < Duration::from_secs(1));
    release.send(()).unwrap();
    let error = responses
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap_err();
    assert_eq!(error.to_string(), "helper startup revoked");
}

#[test]
fn maximum_typed_message_round_trips_through_dispatch_ipc() {
    let empty = serde_json::to_vec(&vec![server_experience::wire::Scalar::Text(String::new())])
        .unwrap()
        .len();
    let record = serde_json::to_vec(&vec![server_experience::wire::Scalar::Text(
        "x".repeat(MAX_MESSAGE_BYTES - empty),
    )])
    .unwrap();
    assert_eq!(record.len(), MAX_MESSAGE_BYTES);
    let request = Dispatch {
        event: Event::Message {
            channel: "f".repeat(MAX_IDENTIFIER_BYTES),
            record,
        },
        epoch: u64::MAX,
        gui: None,
    };
    request.event.check().unwrap();
    assert!(serde_json::to_vec(&request).unwrap().len() > MAX_HOST_OUTPUT);
    let mut bytes = Vec::new();
    write_frame(&mut bytes, &request, MAX_DISPATCH_IPC).unwrap();
    let decoded: Dispatch = read_frame(&mut bytes.as_slice(), MAX_DISPATCH_IPC).unwrap();
    assert_eq!(decoded.event, request.event);
    let Event::Message {
        channel,
        mut record,
    } = request.event
    else {
        unreachable!()
    };
    record.push(b' ');
    assert!(Event::Message { channel, record }.check().is_err());
}

#[test]
fn review_frame_serialization_stops_at_the_byte_limit() {
    use serde::ser::SerializeSeq;
    struct Large<'a>(&'a std::sync::atomic::AtomicUsize);
    impl Serialize for Large<'_> {
        fn serialize<S: serde::Serializer>(
            &self,
            serializer: S,
        ) -> std::result::Result<S::Ok, S::Error> {
            let mut sequence = serializer.serialize_seq(None)?;
            for _ in 0..100 {
                self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                sequence.serialize_element(&"large element".repeat(20))?;
            }
            sequence.end()
        }
    }
    let visits = std::sync::atomic::AtomicUsize::new(0);
    let mut written = Vec::new();
    assert!(write_frame(&mut written, &Large(&visits), 64).is_err());
    assert!(written.is_empty());
    assert_eq!(visits.load(std::sync::atomic::Ordering::Relaxed), 1);
}

#[cfg(feature = "execution")]
#[test]
fn call_failures_name_their_kind_callback_and_bundle_with_a_bounded_clean_reason() {
    let fuel = anyhow::Error::from(wasmtime::Trap::OutOfFuel);
    let failure = CallFailure::of("terminal", "dispatch", &fuel);
    assert_eq!(failure.kind, FailureKind::Fuel);
    assert_eq!(
        (failure.bundle.as_str(), failure.callback.as_str()),
        ("terminal", "dispatch")
    );
    assert!(failure.reason.contains("fuel"), "{}", failure.reason);
    let panic = anyhow::Error::from(wasmtime::Trap::UnreachableCodeReached);
    assert_eq!(
        CallFailure::of("t", "epoch", &panic).kind,
        FailureKind::Panic
    );
    let other = anyhow::Error::from(wasmtime::Trap::StackOverflow);
    assert_eq!(
        CallFailure::of("t", "action", &other).kind,
        FailureKind::Trap
    );
    let refused = anyhow::anyhow!(
        "action not granted\u{1b}[31m\r\n{}",
        "é".repeat(4 * MAX_FAILURE_REASON_BYTES)
    );
    let failure = CallFailure::of("t", "action", &refused);
    assert_eq!(failure.kind, FailureKind::Refused);
    assert!(failure.reason.len() <= MAX_FAILURE_REASON_BYTES);
    assert!(failure.reason.starts_with("action not granted"));
    assert!(
        !failure.reason.chars().any(|c| c.is_control() && c != '\n'),
        "{:?}",
        failure.reason
    );
}

#[test]
fn helper_stderr_lines_are_cut_rate_limited_and_their_drops_counted() {
    let long = "x".repeat(MAX_LOG_LINE_BYTES * 2);
    let mut input = format!("{long}\nshort\n");
    for line in 0..MAX_LOG_LINES_PER_SECOND * 2 {
        input.push_str(&format!("line {line}\n"));
    }
    let (sender, lines) = mpsc::sync_channel(MAX_LOG_LINES_PER_SECOND * 4);
    supervisor::forward_stderr(input.as_bytes(), &sender);
    let lines: Vec<String> = lines.try_iter().collect();
    assert_eq!(lines[0].len(), MAX_LOG_LINE_BYTES);
    assert_eq!(lines[1], "short");
    assert_eq!(lines.len(), MAX_LOG_LINES_PER_SECOND + 1);
    let dropped = MAX_LOG_LINES_PER_SECOND * 2 + 2 - MAX_LOG_LINES_PER_SECOND;
    assert_eq!(
        lines.last().unwrap(),
        &format!("{dropped} more helper stderr lines dropped")
    );
}
