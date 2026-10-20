//! Post-Hello byte pacing through the actual play consumer and its original controller lease.

use super::*;
use bridge::relay_control::Hello;
use std::{io::Cursor, sync::atomic::AtomicUsize};
use tokio::io::{AsyncRead, ReadBuf};

/// Hello consumes none of the initial upload burst, including its retained read-ahead prefix.
#[tokio::test(start_paused = true)]
async fn post_hello_upload_initial_burst_is_available_without_delay() {
    let registry = Registry::new();
    let attachment = registry
        .open_control(Ipv4Addr::LOCALHOST.into(), "", Instant::now())
        .unwrap();
    let (stream, read, total) = input(&attachment, RELAY_UPLOAD_BURST_BYTES);
    let started = tokio::time::Instant::now();
    let result = tokio::time::timeout(Duration::from_secs(1), run(&registry, stream)).await;
    let elapsed = started.elapsed();
    registry.close();
    drop(attachment);

    let error = result
        .expect("the initial post-Hello upload burst was throttled")
        .expect_err("the selected version must be refused without upstream I/O");
    assert_eq!(
        error
            .get_ref()
            .and_then(|error| error.downcast_ref::<ClientDataRefusal>()),
        Some(&ClientDataRefusal::OutdatedServer),
    );
    assert_eq!(read.load(Ordering::Relaxed), total);
    assert_eq!(
        elapsed,
        Duration::ZERO,
        "Hello or its prefix was charged twice"
    );
}

/// Excess bytes remain paced and the actual controller cancellation retires their pending read.
#[tokio::test(start_paused = true)]
async fn post_hello_upload_excess_waits_and_controller_cancel_stops_the_read() {
    let registry = Registry::new();
    let attachment = registry
        .open_control(Ipv4Addr::LOCALHOST.into(), "", Instant::now())
        .unwrap();
    let upload = RELAY_UPLOAD_BURST_BYTES
        .checked_add(usize::try_from(RELAY_UPLOAD_BYTES_PER_SECOND).unwrap())
        .unwrap();
    let (stream, read, _) = input(&attachment, upload);
    let started = tokio::time::Instant::now();
    let (completed_early, read_before_cancel, cancelled_at, retired_at, result) = {
        let serving = run(&registry, stream);
        tokio::pin!(serving);
        let early = match futures::poll!(&mut serving) {
            std::task::Poll::Ready(result) => Some(result),
            std::task::Poll::Pending => {
                tokio::time::advance(Duration::from_millis(500)).await;
                match futures::poll!(&mut serving) {
                    std::task::Poll::Ready(result) => Some(result),
                    std::task::Poll::Pending => None,
                }
            }
        };
        let completed_early = early.is_some();
        let read_before_cancel = read.load(Ordering::Relaxed);
        let cancelled_at = tokio::time::Instant::now();
        registry.close();
        let result = match early {
            Some(result) => result,
            None => (&mut serving).await,
        };
        (
            completed_early,
            read_before_cancel,
            cancelled_at,
            tokio::time::Instant::now(),
            result,
        )
    };
    drop(attachment);

    assert!(
        !completed_early,
        "post-Hello upload exceeded its burst before the generated rate allowed it"
    );
    assert!(
        read_before_cancel >= RELAY_UPLOAD_BURST_BYTES,
        "the real initial burst was not read"
    );
    assert_eq!(
        cancelled_at.duration_since(started),
        Duration::from_millis(500)
    );
    assert_eq!(
        retired_at, cancelled_at,
        "cancellation waited for the pacing timer"
    );
    assert_eq!(
        read.load(Ordering::Relaxed),
        read_before_cancel,
        "cancelled upload read more bytes"
    );
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::BrokenPipe);
}

/// Supplies a real capability and complete generated Connect frame, with no upstream target.
fn input(
    attachment: &crate::registry::Attachment,
    upload: usize,
) -> (
    tokio::io::Join<ObservedRead, tokio::io::Sink>,
    Arc<AtomicUsize>,
    usize,
) {
    let capability = attachment
        .session
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .capability(attachment.generation, false, Instant::now())
        .unwrap();
    let hello = serde_json::to_vec(&Hello {
        version: RELAY_VERSION,
        channel: RELAY_CHANNEL_PLAY.into(),
        channels: Vec::new(),
        resume: capability.resume,
        generation: capability.generation,
        join: 1,
    })
    .unwrap();
    let random = protocol::ClientRandomId::new(1).unwrap();
    let client_data =
        protocol::login_client_data("Fixture", random, None, &protocol::LoginSettings::default());
    let request = session_wire::ConnectRequest {
        protocol: protocol::PROTOCOL_VERSION.checked_add(1).unwrap(),
        join_profile_id: None,
        target: None,
        client_cache: false,
        client_data,
    };
    let mut connect = session_wire::encode_connect(&request).unwrap().to_vec();
    let payload = upload.checked_sub(4).unwrap();
    assert!(connect.len() < payload);
    // Ordinary JSON whitespace counts as upload without enlarging the client-data object.
    connect.resize(payload, b' ');
    assert_eq!(connect.len().checked_add(4).unwrap(), upload);
    assert!(connect.len() < bridge::MAX_FRAME_LEN);
    let decoded = session_wire::decode_native_connect(&connect).unwrap();
    assert_eq!(decoded.protocol, request.protocol);
    let mut wire = Vec::new();
    for frame in [&hello[..], &connect[..]] {
        wire.extend_from_slice(&u32::try_from(frame.len()).unwrap().to_be_bytes());
        wire.extend_from_slice(frame);
    }
    let total = wire.len();
    let read = Arc::new(AtomicUsize::new(0));
    let reader = ObservedRead {
        source: Cursor::new(wire),
        read: Arc::clone(&read),
    };
    (tokio::io::join(reader, tokio::io::sink()), read, total)
}

/// Counts bytes returned by the original finite reader without gating or fabricating readiness.
struct ObservedRead {
    source: Cursor<Vec<u8>>,
    read: Arc<AtomicUsize>,
}

impl AsyncRead for ObservedRead {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        output: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let before = output.filled().len();
        let result = Pin::new(&mut this.source).poll_read(cx, output);
        this.read
            .fetch_add(output.filled().len() - before, Ordering::Relaxed);
        result
    }
}
