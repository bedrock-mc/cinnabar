use crate::capture::{CapturedPacket, hash_packet};
use anyhow::{Result, ensure};
use bytes::Bytes;
use futures::{Sink, SinkExt};
use protocol::session_wire::{
    CapturedPacketKind, CoreMessage, PACK_CHUNK_BYTES, SessionHandoff, encode_core_message,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{ops::Range, time::Duration};
use tokio::time::Instant;

pub struct Burst {
    pub packets: Range<usize>,
    pub bytes: usize,
}

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

/// Keeps startup through StartGame in one handoff, then applies the play burst limits.
pub fn plan_bursts(packets: &[CapturedPacket], count: usize, bytes: usize) -> Result<Vec<Burst>> {
    ensure!(
        count > 0 && bytes > 0,
        "burst packet and byte limits must be positive"
    );
    let start = packets
        .iter()
        .position(|packet| packet.kind == CapturedPacketKind::StartGame)
        .ok_or_else(|| anyhow::anyhow!("replay has no StartGame"))?;
    let mut result = vec![Burst {
        packets: 0..start + 1,
        bytes: packets[..=start]
            .iter()
            .map(|packet| packet.wire.len())
            .sum(),
    }];
    for (index, packet) in packets.iter().enumerate().skip(start + 1) {
        ensure!(
            packet.wire.len() <= bytes,
            "record {} exceeds the burst byte limit",
            packet.record
        );
        if result.len() == 1
            || result.last().unwrap().packets.len() == count
            || result.last().unwrap().bytes + packet.wire.len() > bytes
        {
            result.push(Burst {
                packets: index..index,
                bytes: 0,
            });
        }
        let burst = result.last_mut().unwrap();
        burst.packets.end = index + 1;
        burst.bytes += packet.wire.len();
    }
    Ok(result)
}

/// Flushes each scheduled burst and records only successful writes in the byte witness.
pub async fn replay_bursts<S>(
    sink: &mut S,
    packets: &[CapturedPacket],
    bursts: &[Burst],
    interval: Duration,
    handoff: SessionHandoff,
    archives: &[Bytes],
    result: &mut ReplayResult,
) -> Result<()>
where
    S: Sink<Bytes> + Unpin,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    result.bursts = Some(Vec::with_capacity(bursts.len()));
    let total = interval
        .as_nanos()
        .checked_mul(bursts.len().saturating_sub(1) as u128);
    ensure!(
        total.is_some_and(|total| total <= i64::MAX as u128),
        "burst schedule exceeds the duration range"
    );
    let mut handoff = Some(handoff);
    let mut digest = Sha256::new();
    let started = Instant::now();
    for (index, burst) in bursts.iter().enumerate() {
        let due = Duration::from_nanos((interval.as_nanos() * index as u128) as u64);
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
