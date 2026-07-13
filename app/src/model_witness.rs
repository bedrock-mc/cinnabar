use std::{path::PathBuf, time::Duration};

use anyhow::{Context, Result, ensure};
use bevy::prelude::*;
use render::{ModelWitnessEvidence, ModelWitnessRequest};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use world::SubChunkKey;

const MODEL_WITNESS_SCHEMA: &str = "rust-mcbe-model-witness-v1";
const WITNESS_POLL_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ModelWitnessSubChunk {
    x: i32,
    y: i32,
    z: i32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelWitnessFile {
    schema: String,
    revision: u64,
    dimension: i32,
    request_sha256: String,
    sub_chunks: Vec<ModelWitnessSubChunk>,
}

#[derive(Serialize)]
struct ModelWitnessHashInput<'a> {
    schema: &'a str,
    revision: u64,
    dimension: i32,
    sub_chunks: &'a [ModelWitnessSubChunk],
}

fn canonical_request_hash(file: &ModelWitnessFile) -> Result<[u8; 32]> {
    let canonical = serde_json::to_vec(&ModelWitnessHashInput {
        schema: &file.schema,
        revision: file.revision,
        dimension: file.dimension,
        sub_chunks: &file.sub_chunks,
    })
    .context("encode canonical model witness request")?;
    Ok(Sha256::digest(canonical).into())
}

fn decode_lower_hex_hash(value: &str) -> Result<[u8; 32]> {
    ensure!(
        value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "request_sha256 must be exactly 64 lowercase hexadecimal characters"
    );
    let mut hash = [0_u8; 32];
    for (index, output) in hash.iter_mut().enumerate() {
        *output = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .context("decode request_sha256")?;
    }
    Ok(hash)
}

fn decode_request(bytes: &[u8]) -> Result<ModelWitnessRequest> {
    let file: ModelWitnessFile =
        serde_json::from_slice(bytes).context("decode model witness request JSON")?;
    ensure!(
        file.schema == MODEL_WITNESS_SCHEMA,
        "unsupported model witness schema"
    );
    let declared_hash = decode_lower_hex_hash(&file.request_sha256)?;
    ensure!(
        canonical_request_hash(&file)? == declared_hash,
        "model witness request hash mismatch"
    );
    let keys = file
        .sub_chunks
        .iter()
        .map(|key| SubChunkKey::new(file.dimension, key.x, key.y, key.z))
        .collect();
    ModelWitnessRequest::try_new(file.revision, declared_hash, keys)
        .map_err(|error| anyhow::anyhow!("invalid model witness request: {error:?}"))
}

#[derive(Resource)]
pub struct ModelWitnessFileSource {
    path: Option<PathBuf>,
    next_poll: std::time::Instant,
    last_digest: Option<[u8; 32]>,
    was_missing: bool,
}

impl ModelWitnessFileSource {
    pub fn new(path: Option<PathBuf>) -> Self {
        Self {
            path,
            next_poll: std::time::Instant::now(),
            last_digest: None,
            was_missing: true,
        }
    }
}

pub fn poll_model_witness_request(
    mut source: ResMut<ModelWitnessFileSource>,
    mut request: ResMut<ModelWitnessRequest>,
    evidence: Res<ModelWitnessEvidence>,
) {
    let now = std::time::Instant::now();
    if now < source.next_poll {
        return;
    }
    source.next_poll = now + WITNESS_POLL_INTERVAL;
    let Some(path) = source.path.as_ref() else {
        return;
    };
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if !source.was_missing || request.enabled() {
                *request = ModelWitnessRequest::default();
                evidence.reset();
            }
            source.was_missing = true;
            source.last_digest = None;
            return;
        }
        Err(error) => {
            *request = ModelWitnessRequest::default();
            evidence.reset();
            source.last_digest = None;
            eprintln!("model witness request read failed: {error}");
            return;
        }
    };
    source.was_missing = false;
    let digest: [u8; 32] = Sha256::digest(&bytes).into();
    if source.last_digest == Some(digest) {
        return;
    }
    source.last_digest = Some(digest);
    match decode_request(&bytes) {
        Ok(next) => {
            evidence.set_authoritative_request(&next);
            if *request != next {
                *request = next;
            }
        }
        Err(error) => {
            *request = ModelWitnessRequest::default();
            evidence.reset();
            eprintln!("model witness request rejected: {error:#}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn request_json(revision: u64, sub_chunks: &[ModelWitnessSubChunk]) -> Vec<u8> {
        let file = ModelWitnessFile {
            schema: MODEL_WITNESS_SCHEMA.to_owned(),
            revision,
            dimension: 0,
            request_sha256: String::new(),
            sub_chunks: sub_chunks.to_vec(),
        };
        let hash = canonical_request_hash(&file).unwrap();
        let hash = hash
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        serde_json::to_vec(&serde_json::json!({
            "schema": MODEL_WITNESS_SCHEMA,
            "revision": revision,
            "dimension": 0,
            "request_sha256": hash,
            "sub_chunks": sub_chunks,
        }))
        .unwrap()
    }

    #[test]
    fn request_json_decodes_exact_hash_dimension_keys_and_revision() {
        let bytes = request_json(
            7,
            &[
                ModelWitnessSubChunk { x: 1, y: 4, z: 5 },
                ModelWitnessSubChunk { x: 0, y: 4, z: 5 },
            ],
        );
        let request = decode_request(&bytes).unwrap();
        assert_eq!(request.revision(), 7);
        assert_eq!(
            request.keys(),
            &[SubChunkKey::new(0, 0, 4, 5), SubChunkKey::new(0, 1, 4, 5)]
        );
        assert_ne!(request.request_hash(), &[0; 32]);
    }

    #[test]
    fn request_json_fails_closed_for_hash_schema_duplicates_and_unknown_fields() {
        let valid = request_json(7, &[ModelWitnessSubChunk { x: 1, y: 4, z: 5 }]);
        let mut tampered: serde_json::Value = serde_json::from_slice(&valid).unwrap();
        tampered["request_sha256"] = serde_json::Value::String("0".repeat(64));
        assert!(decode_request(&serde_json::to_vec(&tampered).unwrap()).is_err());

        tampered = serde_json::from_slice(&valid).unwrap();
        tampered["extra"] = serde_json::json!(true);
        assert!(decode_request(&serde_json::to_vec(&tampered).unwrap()).is_err());

        let duplicate = request_json(
            7,
            &[
                ModelWitnessSubChunk { x: 1, y: 4, z: 5 },
                ModelWitnessSubChunk { x: 1, y: 4, z: 5 },
            ],
        );
        assert!(decode_request(&duplicate).is_err());
    }

    #[test]
    fn file_poller_retries_same_bytes_after_non_not_found_read_error() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "rust-mcbe-model-witness-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("request.json");
        let valid = request_json(7, &[ModelWitnessSubChunk { x: 1, y: 4, z: 5 }]);
        std::fs::write(&path, &valid).unwrap();

        let mut app = App::new();
        app.insert_resource(ModelWitnessFileSource::new(Some(path.clone())))
            .init_resource::<ModelWitnessRequest>()
            .init_resource::<ModelWitnessEvidence>()
            .add_systems(Update, poll_model_witness_request);
        app.update();
        assert_eq!(app.world().resource::<ModelWitnessRequest>().revision(), 7);

        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        app.world_mut()
            .resource_mut::<ModelWitnessFileSource>()
            .next_poll = std::time::Instant::now();
        app.update();
        assert_eq!(app.world().resource::<ModelWitnessRequest>().revision(), 0);

        std::fs::remove_dir(&path).unwrap();
        std::fs::write(&path, &valid).unwrap();
        app.world_mut()
            .resource_mut::<ModelWitnessFileSource>()
            .next_poll = std::time::Instant::now();
        app.update();
        assert_eq!(app.world().resource::<ModelWitnessRequest>().revision(), 7);

        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir_all(directory).unwrap();
    }
}
