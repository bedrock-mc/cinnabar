use crate::capture::{CapturedPacket, hash_packet};
use anyhow::{Result, ensure};
use bridge::contract::PACK_CHUNK_BYTES;
use bytes::Bytes;
use futures::{Sink, SinkExt};
use protocol::session_wire::{
    CapturedPacketKind, CoreMessage, SessionHandoff, encode_core_message,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{ops::Range, time::Duration};
use tokio::time::Instant;

pub struct Burst {
    pub packets: Range<usize>,
    pub bytes: usize,
    /// When the burst is sent, from the handoff.
    pub due: Duration,
}

/// When each burst is sent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Schedule {
    /// Bursts `interval` apart from the handoff on.
    Fixed(Duration),
    /// Each burst when its first packet was originally received, counted from StartGame's.
    /// Packets received within [`ORIGINAL_TIMING_RESOLUTION`] of a burst's first packet join
    /// it, within the burst limits. The client records a packet when its session read the
    /// batch from the core, which trails socket arrival while its ingress is backlogged, so
    /// this replays the original client's read pace, slowdowns included.
    Original,
}

/// How close in recorded arrival time packets must be to share an original-timing burst.
pub const ORIGINAL_TIMING_RESOLUTION: Duration = Duration::from_millis(1);

#[derive(Default, Serialize)]
pub struct ReplayResult {
    pub packets: usize,
    pub sha256: String,
    pub bursts: Option<Vec<BurstSample>>,
}

#[derive(Serialize)]
pub struct BurstSample {
    pub first_record: usize,
    pub last_record: usize,
    pub packets: usize,
    pub bytes: usize,
    pub due_ms: f64,
    pub flushed_ms: f64,
}

/// Keeps startup through StartGame in one handoff, then applies the play burst limits and the
/// schedule.
pub fn plan_bursts(
    packets: &[CapturedPacket],
    count: usize,
    bytes: usize,
    schedule: Schedule,
) -> Result<Vec<Burst>> {
    ensure!(
        count > 0 && bytes > 0,
        "burst packet and byte limits must be positive"
    );
    let start = packets
        .iter()
        .position(|packet| packet.kind == CapturedPacketKind::StartGame)
        .ok_or_else(|| anyhow::anyhow!("replay has no StartGame"))?;
    let start_elapsed = match schedule {
        Schedule::Fixed(_) => Duration::ZERO,
        Schedule::Original => {
            ensure!(
                packets.iter().all(|packet| packet.elapsed.is_some()),
                "original timing needs a capture with recorded times (v2)"
            );
            packets[start].elapsed.unwrap_or_default()
        }
    };
    let mut result = vec![Burst {
        packets: 0..start + 1,
        bytes: packets[..=start]
            .iter()
            .map(|packet| packet.wire.len())
            .sum(),
        due: Duration::ZERO,
    }];
    for (index, packet) in packets.iter().enumerate().skip(start + 1) {
        ensure!(
            packet.wire.len() <= bytes,
            "record {} exceeds the burst byte limit",
            packet.record
        );
        let last = result.last().unwrap();
        let arrived = packet.elapsed.unwrap_or_default();
        let later = schedule == Schedule::Original
            && arrived.saturating_sub(packets[last.packets.start].elapsed.unwrap_or_default())
                >= ORIGINAL_TIMING_RESOLUTION;
        if result.len() == 1
            || later
            || last.packets.len() == count
            || last.bytes + packet.wire.len() > bytes
        {
            let due = match schedule {
                Schedule::Fixed(interval) => interval.checked_mul(u32::try_from(result.len())?),
                // Arrival times never run backwards, but a schedule must not either.
                Schedule::Original => Some(arrived.saturating_sub(start_elapsed).max(last.due)),
            }
            .filter(|due| due.as_nanos() <= i64::MAX as u128)
            .ok_or_else(|| anyhow::anyhow!("burst schedule exceeds the duration range"))?;
            result.push(Burst {
                packets: index..index,
                bytes: 0,
                due,
            });
        }
        let burst = result.last_mut().unwrap();
        burst.packets.end = index + 1;
        burst.bytes += packet.wire.len();
    }
    Ok(result)
}

/// Flushes each burst at its due time and records only successful writes in the byte witness.
pub async fn replay_bursts<S>(
    sink: &mut S,
    packets: &[CapturedPacket],
    bursts: &[Burst],
    handoff: SessionHandoff,
    archives: &[Bytes],
    result: &mut ReplayResult,
) -> Result<()>
where
    S: Sink<Bytes> + Unpin,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    result.bursts = Some(Vec::with_capacity(bursts.len()));
    let mut handoff = Some(handoff);
    let mut digest = Sha256::new();
    let started = Instant::now();
    for burst in bursts {
        let due = burst.due;
        tokio::time::sleep_until(started + due).await;
        let wire: Vec<_> = packets[burst.packets.clone()]
            .iter()
            .map(|packet| packet.wire.clone())
            .collect();
        if let Some(mut handoff) = handoff.take() {
            handoff.startup = wire;
            sink.send(encode_core_message(&CoreMessage::Handoff(handoff))?)
                .await?;
            for (index, archive) in archives.iter().enumerate() {
                for offset in (0..archive.len()).step_by(PACK_CHUNK_BYTES) {
                    let data =
                        archive.slice(offset..(offset + PACK_CHUNK_BYTES).min(archive.len()));
                    sink.send(encode_core_message(&CoreMessage::PackData {
                        index: u32::try_from(index)?,
                        data,
                    })?)
                    .await?;
                }
            }
        } else {
            sink.send(encode_core_message(&CoreMessage::Batch(wire))?)
                .await?;
        }
        for packet in &packets[burst.packets.clone()] {
            hash_packet(&mut digest, &packet.wire);
        }
        result.packets += burst.packets.len();
        result.bursts.as_mut().unwrap().push(BurstSample {
            first_record: packets[burst.packets.start].record,
            last_record: packets[burst.packets.end - 1].record,
            packets: burst.packets.len(),
            bytes: burst.bytes,
            due_ms: due.as_secs_f64() * 1000.,
            flushed_ms: started.elapsed().as_secs_f64() * 1000.,
        });
    }
    result.sha256 = format!("{:x}", digest.finalize());
    Ok(())
}
