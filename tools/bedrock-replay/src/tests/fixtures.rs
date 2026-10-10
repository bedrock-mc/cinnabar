use protocol::wire::valentine::bedrock::version::v1_26_51::{
    ChunkRadiusUpdatedPacket, EnumsPlayStatus, ItemRegistryPacket, PlayStatusPacket,
};
use protocol::{
    BedrockSession,
    session_wire::{CoreMessage, batch_frame_from_bedrock, decode_core_message},
};
use sha2::{Digest, Sha256};
use std::{fs::OpenOptions, io::Write, path::Path};

/// Writes the historical little-endian capture envelope.
pub(super) fn record(id: u32, body: &[u8]) -> Vec<u8> {
    let mut result = Vec::new();
    result.extend_from_slice(&id.to_le_bytes());
    result.extend_from_slice(&(body.len() as u32).to_le_bytes());
    result.extend_from_slice(body);
    result
}

/// Removes only the packet's subclient header and preserves its original body.
fn unframe(batch: &[u8]) -> Vec<u8> {
    let CoreMessage::Batch(packets) =
        decode_core_message(batch_frame_from_bedrock(batch).unwrap()).unwrap()
    else {
        panic!("batch");
    };
    assert_eq!(packets.len(), 1);
    let packet = &packets[0];
    let mut id = 0u32;
    let mut length = 0;
    for &byte in packet.iter().take(5) {
        id |= u32::from(byte & 0x7f) << (7 * length);
        length += 1;
        if byte & 0x80 == 0 {
            break;
        }
    }
    record(id & 0x3ff, &packet[length..])
}

/// Verifies the checked-in fixture hash before removing its batch and subclient envelope.
fn committed(name: &str) -> Vec<u8> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/protocol/fixtures");
    let entries: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("manifest.json")).unwrap()).unwrap();
    let entry = entries
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["file"] == name)
        .unwrap();
    let data = std::fs::read(root.join(name)).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&data)),
        entry["sha256"].as_str().unwrap()
    );
    let result = unframe(&data);
    assert_eq!(
        u32::from_le_bytes(result[..4].try_into().unwrap()) as u64,
        entry["id"].as_u64().unwrap()
    );
    result
}

/// Combines pinned packet fixtures with the minimal original spawn-completion packets.
pub(super) fn capture() -> Vec<u8> {
    let mut result = committed("start_game.bin");
    let packets: [protocol::Packet; 3] = [
        ItemRegistryPacket::default().into(),
        ChunkRadiusUpdatedPacket { chunk_radius: 1 }.into(),
        PlayStatusPacket {
            status: EnumsPlayStatus::Playerspawn,
        }
        .into(),
    ];
    for packet in packets {
        result.extend(unframe(
            &protocol::encode(&packet, &BedrockSession { shield_item_id: 0 }).unwrap(),
        ));
    }
    result.extend(committed("add_actor.bin"));
    result.extend(committed("text.bin"));
    result
}

/// Builds an original resource pack; a wrapper exercises the existing one-level ZIP behavior.
pub(super) fn pack(nested: bool) -> Vec<u8> {
    let manifest = br#"{"format_version":2,"header":{"name":"Replay test","description":"Synthetic fixture","uuid":"87ad314d-1b7e-4a06-8a7d-882798c777cf","version":[1,0,0]},"modules":[{"type":"resources","uuid":"86fad15a-f775-4d45-bd27-bd2e420a3325","version":[1,0,0]}]}"#;
    let bytes = archive("manifest.json", manifest);
    if nested {
        archive("inner.zip", &bytes)
    } else {
        bytes
    }
}

/// Builds a ZIP without changing the supplied file bytes.
fn archive(name: &str, data: &[u8]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    zip.start_file(name, zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(data).unwrap();
    zip.finish().unwrap().into_inner()
}

/// Creates an export exclusively so an earlier diagnostic capture cannot be replaced.
fn export(path: &Path) -> std::io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)?.write_all(&capture())
}

#[test]
fn fixture_export_preserves_bytes_and_existing_evidence() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("raw.bin");
    export(&path).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(capture())),
        "26b857d1e6295c44065d01b4fd31714c7329916c3741011b10bfeca2d34d077a"
    );
    assert_eq!(std::fs::read(&path).unwrap(), capture());
    assert_eq!(
        export(&path).unwrap_err().kind(),
        std::io::ErrorKind::AlreadyExists
    );
}

#[test]
fn export_replay_fixture() {
    let Some(path) = std::env::var_os("CINNABAR_REPLAY_FIXTURE_OUT") else {
        eprintln!(
            "missing fixture export destination: CINNABAR_REPLAY_FIXTURE_OUT; skipping export"
        );
        return;
    };
    export(Path::new(&path)).unwrap();
}
