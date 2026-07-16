use crate::*;

use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use render::{TransparentWitnessEvidence, TransparentWitnessRequest};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use world::SubChunkKey;

pub(crate) const WITNESS_SCHEMA: &str = "rust-mcbe-transparent-witness-v1";
pub(crate) const MAX_WITNESS_FILE_BYTES: u64 = 16 * 1024;
pub(crate) const WITNESS_POLL_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WitnessFile {
    pub(crate) schema: String,
    pub(crate) revision: u64,
    pub(crate) dimension: i32,
    pub(crate) sub_chunks: Vec<WitnessSubChunk>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WitnessSubChunk {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) z: i32,
}

pub(crate) fn decode_request(bytes: &[u8]) -> Result<TransparentWitnessRequest> {
    if bytes.len() as u64 > MAX_WITNESS_FILE_BYTES {
        bail!("transparent witness request exceeds {MAX_WITNESS_FILE_BYTES} bytes");
    }
    let file: WitnessFile =
        serde_json::from_slice(bytes).context("decode transparent witness request JSON")?;
    if file.schema != WITNESS_SCHEMA {
        bail!("unsupported transparent witness request schema");
    }
    let keys = file
        .sub_chunks
        .into_iter()
        .map(|key| SubChunkKey::new(file.dimension, key.x, key.y, key.z))
        .collect();
    TransparentWitnessRequest::try_new(file.revision, keys)
        .map_err(|error| anyhow::anyhow!("invalid transparent witness request: {error:?}"))
}

#[derive(Resource, Debug)]
pub struct TransparentWitnessFileSource {
    path: Option<PathBuf>,
    next_poll: Instant,
    last_digest: Option<[u8; 32]>,
    was_missing: bool,
}

impl TransparentWitnessFileSource {
    pub fn new(path: Option<PathBuf>) -> Self {
        Self {
            path,
            next_poll: Instant::now(),
            last_digest: None,
            was_missing: true,
        }
    }
}

pub fn poll_transparent_witness_request(
    mut source: ResMut<TransparentWitnessFileSource>,
    mut request: ResMut<TransparentWitnessRequest>,
    evidence: Res<TransparentWitnessEvidence>,
) {
    let now = Instant::now();
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
            if !source.was_missing || request.revision() != 0 {
                *request = TransparentWitnessRequest::default();
                evidence.reset();
            }
            source.was_missing = true;
            source.last_digest = None;
            return;
        }
        Err(error) => {
            *request = TransparentWitnessRequest::default();
            evidence.reset();
            eprintln!("transparent witness request read failed: {error}");
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
            *request = TransparentWitnessRequest::default();
            evidence.reset();
            eprintln!("transparent witness request rejected: {error:#}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    pub(crate) fn request_json_decodes_exact_dimension_keys_and_revision() {
        let request = decode_request(
            br#"{"schema":"rust-mcbe-transparent-witness-v1","revision":7,"dimension":0,"sub_chunks":[{"x":1,"y":4,"z":5},{"x":0,"y":4,"z":5}]}"#,
        )
        .unwrap();
        assert_eq!(request.revision(), 7);
        assert_eq!(
            request.keys(),
            &[SubChunkKey::new(0, 0, 4, 5), SubChunkKey::new(0, 1, 4, 5)]
        );
    }

    #[test]
    pub(crate) fn request_json_fails_closed_for_schema_duplicates_empty_and_excess() {
        for json in [
            r#"{"schema":"wrong","revision":1,"dimension":0,"sub_chunks":[{"x":0,"y":0,"z":0}]}"#.to_owned(),
            r#"{"schema":"rust-mcbe-transparent-witness-v1","revision":1,"dimension":0,"sub_chunks":[]}"#.to_owned(),
            r#"{"schema":"rust-mcbe-transparent-witness-v1","revision":1,"dimension":0,"sub_chunks":[{"x":0,"y":0,"z":0},{"x":0,"y":0,"z":0}]}"#.to_owned(),
            format!(
                "{{\"schema\":\"rust-mcbe-transparent-witness-v1\",\"revision\":1,\"dimension\":0,\"sub_chunks\":[{}]}}",
                (0..=render::MAX_TRANSPARENT_WITNESS_KEYS)
                    .map(|x| format!("{{\"x\":{x},\"y\":0,\"z\":0}}"))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        ] {
            assert!(decode_request(json.as_bytes()).is_err());
        }
    }

    #[test]
    pub(crate) fn file_poller_resets_fail_closed_on_malformed_or_missing_request() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "rust-mcbe-transparent-witness-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("request.json");
        let valid = br#"{"schema":"rust-mcbe-transparent-witness-v1","revision":7,"dimension":0,"sub_chunks":[{"x":1,"y":4,"z":5}]}"#;
        std::fs::write(&path, valid).unwrap();

        let mut app = App::new();
        app.insert_resource(TransparentWitnessFileSource::new(Some(path.clone())))
            .init_resource::<TransparentWitnessRequest>()
            .init_resource::<TransparentWitnessEvidence>()
            .add_systems(Update, poll_transparent_witness_request);
        app.update();
        assert_eq!(
            app.world()
                .resource::<TransparentWitnessRequest>()
                .revision(),
            7
        );

        std::fs::write(&path, b"not-json").unwrap();
        app.world_mut()
            .resource_mut::<TransparentWitnessFileSource>()
            .next_poll = Instant::now();
        app.update();
        assert_eq!(
            app.world()
                .resource::<TransparentWitnessRequest>()
                .revision(),
            0
        );

        std::fs::write(&path, valid).unwrap();
        app.world_mut()
            .resource_mut::<TransparentWitnessFileSource>()
            .next_poll = Instant::now();
        app.update();
        assert_eq!(
            app.world()
                .resource::<TransparentWitnessRequest>()
                .revision(),
            7
        );

        std::fs::remove_file(&path).unwrap();
        app.world_mut()
            .resource_mut::<TransparentWitnessFileSource>()
            .next_poll = Instant::now();
        app.update();
        assert_eq!(
            app.world()
                .resource::<TransparentWitnessRequest>()
                .revision(),
            0
        );
        std::fs::remove_dir_all(directory).unwrap();
    }
}
