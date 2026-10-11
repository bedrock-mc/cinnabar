use crate::{
    capture::{CaptureSummary, read_capture},
    options::Options,
    packs::{PackSummary, read_pack},
    replay::{Burst, ReplayResult, plan_bursts, replay_bursts},
};
use anyhow::{Result, bail, ensure};
use bridge::SessionListener;
use futures::StreamExt;
use protocol::session_wire::{SessionHandoff, SessionIdentity, decode_connect};
use serde::Serialize;
use std::{
    fs::{File, OpenOptions},
    future::Future,
    io::Write,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Serialize)]
struct RunReport {
    protocol: i32,
    version: &'static str,
    capture: CaptureSummary,
    packs: Option<Vec<PackSummary>>,
    burst_packets: usize,
    burst_bytes: usize,
    interval_ms: f64,
    hold_ms: f64,
    started_unix_ms: u128,
    replay_started_unix_ms: u128,
    finished_unix_ms: u128,
    replay: ReplayResult,
    complete: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    end_reason: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    error: String,
}

/// Captures whole-run cancellation in the report and never overwrites previous evidence.
pub async fn run(
    options: Options,
    output: &mut impl Write,
    cancelled: impl Future<Output = ()>,
) -> Result<()> {
    let deadline = tokio::time::Instant::now() + options.timeout;
    let captured = read_capture(File::open(&options.capture)?)?;
    let bursts = plan_bursts(
        &captured.packets,
        options.burst_packets,
        options.burst_bytes,
    )?;
    let mut open = OpenOptions::new();
    open.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        open.mode(0o600);
    }
    let mut file = open.open(&options.report)?;
    let mut report = RunReport {
        protocol: protocol::PROTOCOL_VERSION,
        version: protocol::GAME_VERSION,
        capture: captured.summary,
        packs: None,
        burst_packets: options.burst_packets,
        burst_bytes: options.burst_bytes,
        interval_ms: options.interval.as_secs_f64() * 1000.,
        hold_ms: options.hold.as_secs_f64() * 1000.,
        started_unix_ms: unix_ms(),
        replay_started_unix_ms: 0,
        finished_unix_ms: 0,
        replay: ReplayResult::default(),
        complete: false,
        end_reason: String::new(),
        error: String::new(),
    };
    let result = tokio::select! { biased;
        _ = cancelled => Err(anyhow::anyhow!("context canceled")),
        _ = tokio::time::sleep_until(deadline) => Err(anyhow::anyhow!("context deadline exceeded")),
        result = serve(&options, captured.packets, bursts, &mut report, output) => result,
    };
    report.finished_unix_ms = unix_ms();
    if let Err(error) = &result {
        report.error = format!("{error:#}");
    }
    serde_json::to_writer_pretty(&mut file, &report)?;
    file.write_all(b"\n")?;
    file.flush()?;
    result
}

/// Answers one Connect, sends startup and archives, and drains client frames until the chosen end.
async fn serve(
    options: &Options,
    packets: Vec<crate::capture::CapturedPacket>,
    bursts: Vec<Burst>,
    report: &mut RunReport,
    output: &mut impl Write,
) -> Result<()> {
    let mut metadata = Vec::new();
    let mut archives = Vec::new();
    for path in &options.packs {
        let pack = read_pack(path)?;
        metadata.push(pack.metadata);
        archives.push(pack.archive);
        report.packs.get_or_insert_default().push(pack.summary);
    }
    let listener = SessionListener::bind(&options.socket_dir).await?;
    writeln!(
        output,
        "BEDROCK_REPLAY_READY {}",
        options.socket_dir.display()
    )?;
    output.flush()?;
    let mut stream = listener.accept().await?;
    let frame = stream
        .next()
        .await
        .ok_or_else(|| anyhow::anyhow!("client closed before Connect"))??;
    let request = decode_connect(&frame)?;
    ensure!(
        request.protocol == protocol::PROTOCOL_VERSION
            && request.client_data["GameVersion"] == protocol::GAME_VERSION,
        "unsupported replay protocol"
    );
    let handoff = SessionHandoff {
        identity: SessionIdentity {
            display_name: request.client_data["ThirdPartyName"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            xuid: String::new(),
            uuid: String::new(),
        },
        client_cache: false,
        packs_required: false,
        packs: metadata,
        startup: Vec::new(),
    };
    let (mut sink, mut reader) = stream.split();
    let receive = async {
        while let Some(frame) = reader.next().await {
            frame?;
        }
        Ok::<_, bridge::BridgeError>(())
    };
    tokio::pin!(receive);
    report.replay_started_unix_ms = unix_ms();
    tokio::select! { biased;
        result = replay_bursts(&mut sink, &packets, &bursts, options.interval, handoff, &archives, &mut report.replay) => result?,
        result = &mut receive => { result?; bail!("client closed during replay"); }
    }
    report.complete = report.replay.sha256 == report.capture.replay_sha256
        && report.replay.packets == report.capture.replay_packets;
    ensure!(
        report.complete,
        "replayed packet witness differs from the capture"
    );
    writeln!(
        output,
        "BEDROCK_REPLAY_DELIVERED {} {}",
        report.replay.packets, report.replay.sha256
    )?;
    output.flush()?;
    report.end_reason = tokio::select! {
        result = &mut receive => { result?; "client_exit" },
        _ = tokio::time::sleep(options.hold), if !options.hold.is_zero() => "fixture_end",
    }
    .to_owned();
    Ok(())
}

/// Returns wall-clock report timestamps independently of the monotonic burst schedule.
fn unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
