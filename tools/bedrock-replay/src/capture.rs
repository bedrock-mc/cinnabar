use anyhow::{Context, Result, ensure};
use bytes::Bytes;
use protocol::capture::{CaptureRecord, MAX_CAPTURE_BYTES, is_capture_v2};
use protocol::packet_tap::PacketDirection;
use protocol::session_wire::{
    CapturedPacketKind, captured_packet_kind, packet_from_body, validate_replay_cache_mode,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io::Read, time::Duration};

pub struct CapturedPacket {
    pub record: usize,
    pub kind: CapturedPacketKind,
    pub wire: Bytes,
    /// When the client received the packet, from the capture's start; v1 records none.
    pub elapsed: Option<Duration>,
}
pub struct Capture {
    pub packets: Vec<CapturedPacket>,
    pub summary: CaptureSummary,
}

#[derive(Serialize)]
pub struct CaptureSummary {
    pub format: u16,
    pub sha256: String,
    pub replay_sha256: String,
    pub bytes: usize,
    pub records: usize,
    pub handshake_records_regenerated: usize,
    pub replay_packets: usize,
    pub packet_counts: BTreeMap<u32, usize>,
    pub serverbound_records_ignored: usize,
    pub dropped_packets: u64,
    /// The capture ended early, at its size limit or inside a record its writer never finished;
    /// replay keeps the complete records.
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capture_started_unix_ms: Option<u64>,
    pub original_timing: &'static str,
}

/// One clientbound packet as either format stores it.
struct Entry {
    record: usize,
    id: u32,
    body: Bytes,
    elapsed: Option<Duration>,
}

/// Validates a whole capture, v1 or v2, before publishing any endpoint.
///
/// v1 repeats a little-endian `u32` packet ID, `u32` body length and the body. v2 starts with
/// the `CNBRCAP2` magic; its serverbound records are counted and ignored.
pub fn read_capture(reader: impl Read) -> Result<Capture> {
    let mut data = Vec::new();
    reader.take(MAX_CAPTURE_BYTES + 1).read_to_end(&mut data)?;
    ensure!(
        data.len() as u64 <= MAX_CAPTURE_BYTES,
        "capture exceeds the 512 MiB fixture limit"
    );
    let mut summary = CaptureSummary {
        format: 1,
        sha256: format!("{:x}", Sha256::digest(&data)),
        replay_sha256: String::new(),
        bytes: data.len(),
        records: 0,
        handshake_records_regenerated: 0,
        replay_packets: 0,
        packet_counts: BTreeMap::new(),
        serverbound_records_ignored: 0,
        dropped_packets: 0,
        truncated: false,
        capture_started_unix_ms: None,
        original_timing: "not recorded; fixed burst schedule",
    };
    let data = Bytes::from(data);
    let entries = if is_capture_v2(&data) {
        read_v2(data, &mut summary)?
    } else {
        read_v1(&data, &mut summary)?
    };
    let mut packets = Vec::new();
    let mut started = false;
    let mut digest = Sha256::new();
    for Entry {
        record,
        id,
        body,
        elapsed,
    } in entries
    {
        ensure!(
            id <= 0x3ff,
            "record {record}: packet ID does not fit the Bedrock header"
        );
        let kind = captured_packet_kind(id);
        ensure!(
            kind != CapturedPacketKind::Transfer,
            "record {record}: server transfer is not allowed in offline replay"
        );
        if kind == CapturedPacketKind::StartGame {
            ensure!(
                !started,
                "record {record}: replay requires exactly one StartGame"
            );
            started = true;
        }
        if !started && kind == CapturedPacketKind::Login {
            summary.handshake_records_regenerated += 1;
            continue;
        }
        validate_replay_cache_mode(id, &body).with_context(|| format!("record {record}"))?;
        let wire = packet_from_body(id, &body)?;
        hash_packet(&mut digest, &wire);
        packets.push(CapturedPacket {
            record,
            kind,
            wire,
            elapsed,
        });
        *summary.packet_counts.entry(id).or_default() += 1;
    }
    ensure!(started, "capture has no StartGame");
    summary.replay_packets = packets.len();
    summary.replay_sha256 = format!("{:x}", digest.finalize());
    Ok(Capture { packets, summary })
}

/// Splits a v1 capture; bodies stay views of `data`.
fn read_v1(data: &Bytes, summary: &mut CaptureSummary) -> Result<Vec<Entry>> {
    let mut entries = Vec::new();
    let mut offset = 0;
    while offset < data.len() {
        let record = summary.records;
        let rest = &data[offset..];
        ensure!(rest.len() >= 8, "record {record}: truncated capture header");
        let id = u32::from_le_bytes(rest[..4].try_into()?);
        let length = u32::from_le_bytes(rest[4..8].try_into()?) as usize;
        ensure!(
            length <= rest.len() - 8,
            "record {record}: truncated capture body"
        );
        let start = offset + 8;
        offset = start + length;
        summary.records += 1;
        entries.push(Entry {
            record,
            id,
            body: data.slice(start..offset),
            elapsed: None,
        });
    }
    Ok(entries)
}

/// Reads a v2 capture, keeping clientbound packets with their arrival times and counting what
/// replay leaves out.
fn read_v2(data: Bytes, summary: &mut CaptureSummary) -> Result<Vec<Entry>> {
    let file = protocol::capture::read_capture(data)?;
    let header = file.header;
    ensure!(
        header.protocol_version == protocol::PROTOCOL_VERSION,
        "capture protocol {} differs from the pinned protocol {}",
        header.protocol_version,
        protocol::PROTOCOL_VERSION
    );
    let records = file.records;
    summary.format = 2;
    summary.records = records.len();
    summary.truncated = file.cut_short;
    summary.capture_started_unix_ms = Some(header.started_unix_ms);
    summary.original_timing = "recorded";
    let mut entries = Vec::new();
    for (record, entry) in records.into_iter().enumerate() {
        match entry {
            CaptureRecord::Packet {
                direction: PacketDirection::Clientbound,
                packet_id,
                elapsed_us,
                body,
            } => entries.push(Entry {
                record,
                id: packet_id,
                body,
                elapsed: Some(Duration::from_micros(elapsed_us)),
            }),
            CaptureRecord::Packet {
                direction: PacketDirection::Serverbound,
                ..
            } => summary.serverbound_records_ignored += 1,
            CaptureRecord::Gap { dropped, .. } => {
                summary.dropped_packets = summary.dropped_packets.saturating_add(dropped);
            }
            CaptureRecord::Truncated { .. } => summary.truncated = true,
        }
    }
    Ok(entries)
}

/// Includes packet lengths so distinct partitions cannot share an ordered byte witness.
pub fn hash_packet(digest: &mut Sha256, wire: &[u8]) {
    digest.update((wire.len() as u32).to_le_bytes());
    digest.update(wire);
}
