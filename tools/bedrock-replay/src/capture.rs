use anyhow::{Result, ensure};
use bytes::Bytes;
use protocol::session_wire::{CapturedPacketKind, captured_packet_kind, packet_from_body};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io::Read};

const MAX_CAPTURE_BYTES: u64 = 512 << 20;

pub struct CapturedPacket {
    pub record: usize,
    pub kind: CapturedPacketKind,
    pub wire: Bytes,
}
pub struct Capture {
    pub packets: Vec<CapturedPacket>,
    pub summary: CaptureSummary,
}

#[derive(Serialize)]
pub struct CaptureSummary {
    pub sha256: String,
    pub replay_sha256: String,
    pub bytes: usize,
    pub records: usize,
    pub handshake_records_regenerated: usize,
    pub replay_packets: usize,
    pub packet_counts: BTreeMap<u32, usize>,
    pub original_timing: &'static str,
}

/// Validates the complete id/length/body capture before publishing any endpoint.
pub fn read_capture(reader: impl Read) -> Result<Capture> {
    let mut data = Vec::new();
    reader.take(MAX_CAPTURE_BYTES + 1).read_to_end(&mut data)?;
    ensure!(
        data.len() as u64 <= MAX_CAPTURE_BYTES,
        "capture exceeds the 512 MiB fixture limit"
    );
    let mut result = Capture {
        packets: Vec::new(),
        summary: CaptureSummary {
            sha256: format!("{:x}", Sha256::digest(&data)),
            replay_sha256: String::new(),
            bytes: data.len(),
            records: 0,
            handshake_records_regenerated: 0,
            replay_packets: 0,
            packet_counts: BTreeMap::new(),
            original_timing: "not recorded; fixed burst schedule",
        },
    };
    let mut started = false;
    let mut digest = Sha256::new();
    let mut rest = data.as_slice();
    while !rest.is_empty() {
        let record = result.summary.records;
        ensure!(rest.len() >= 8, "record {record}: truncated capture header");
        let id = u32::from_le_bytes(rest[..4].try_into()?);
        let length = u32::from_le_bytes(rest[4..8].try_into()?) as usize;
        rest = &rest[8..];
        ensure!(
            length <= rest.len(),
            "record {record}: truncated capture body"
        );
        let (body, tail) = rest.split_at(length);
        rest = tail;
        result.summary.records += 1;
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
            result.summary.handshake_records_regenerated += 1;
            continue;
        }
        let wire = packet_from_body(id, body)?;
        hash_packet(&mut digest, &wire);
        result.packets.push(CapturedPacket { record, kind, wire });
        *result.summary.packet_counts.entry(id).or_default() += 1;
    }
    ensure!(started, "capture has no StartGame");
    result.summary.replay_packets = result.packets.len();
    result.summary.replay_sha256 = format!("{:x}", digest.finalize());
    Ok(result)
}

/// Includes packet lengths so distinct partitions cannot share an ordered byte witness.
pub fn hash_packet(digest: &mut Sha256, wire: &[u8]) {
    digest.update((wire.len() as u32).to_le_bytes());
    digest.update(wire);
}
