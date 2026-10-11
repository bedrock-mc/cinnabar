use anyhow::{Context, Result, ensure};
use bytes::Bytes;
use protocol::session_wire::{HandoffPack, PackContentKey};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    io::{Cursor, Read},
    path::Path,
};

pub struct Pack {
    pub metadata: HandoffPack,
    pub archive: Bytes,
    pub summary: PackSummary,
}

#[derive(Serialize)]
pub struct PackSummary {
    #[serde(rename = "UUID")]
    pub uuid: String,
    #[serde(rename = "Version")]
    pub version: String,
    #[serde(rename = "SHA256")]
    pub sha256: String,
}

/// Reads a local archive, unwrapping one nested ZIP and retaining the input file's report hash.
pub fn read_pack(path: &Path) -> Result<Pack> {
    let mut bytes = std::fs::read(path)?;
    let sha256 = format!("{:x}", Sha256::digest(&bytes));
    let mut zip = zip::ZipArchive::new(Cursor::new(&bytes))?;
    if zip.len() == 1 && zip.by_index(0)?.name().to_lowercase().ends_with(".zip") {
        let mut nested = Vec::new();
        zip.by_index(0)?.read_to_end(&mut nested)?;
        drop(zip);
        bytes = nested;
        zip = zip::ZipArchive::new(Cursor::new(&bytes))?;
    }
    let index = (0..zip.len())
        .find(|&index| {
            zip.by_index(index)
                .is_ok_and(|entry| entry.name().rsplit('/').next() == Some("manifest.json"))
        })
        .context("pack has no manifest.json")?;
    let mut manifest = Vec::new();
    zip.by_index(index)?.read_to_end(&mut manifest)?;
    let normalized = resource_pack::normalize_jsonc(&manifest).context("invalid pack manifest")?;
    let manifest: Value = serde_json::from_slice(&normalized)?;
    let header = &manifest["header"];
    let uuid = match header.get("uuid").and_then(Value::as_str) {
        Some(value) => uuid::Uuid::parse_str(value)?,
        None => uuid::Uuid::nil(),
    }
    .to_string();
    let mut components = [0i64; 3];
    match &header["version"] {
        Value::Null => {}
        Value::Array(values) => {
            for (slot, value) in components.iter_mut().zip(values) {
                *slot = value.as_i64().context("invalid pack version")?;
            }
        }
        Value::String(value) => {
            let values: Vec<_> = value.trim().split('.').collect();
            ensure!(values.len() == 3, "pack version needs three components");
            for (slot, value) in components.iter_mut().zip(values) {
                *slot = value.parse()?;
            }
        }
        _ => anyhow::bail!("invalid pack version"),
    }
    let version = components.map(|value| value.to_string()).join(".");
    drop(zip);
    Ok(Pack {
        metadata: HandoffPack {
            uuid: uuid.clone(),
            version: version.clone(),
            sub_pack: String::new(),
            content_key: PackContentKey::new(String::new()),
            size: bytes.len() as u64,
            cache: None,
        },
        archive: bytes.into(),
        summary: PackSummary {
            uuid,
            version,
            sha256,
        },
    })
}
