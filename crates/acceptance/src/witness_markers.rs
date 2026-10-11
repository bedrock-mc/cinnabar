//! Renderer witness evidence generated from read-only observations.
use diagnostics::{
    markers::{
        MODEL_WITNESS_COMPLETE, TRANSPARENT_WITNESS_COMPLETE, TRANSPARENT_WITNESS_INCOMPLETE,
        TRANSPARENT_WITNESS_STAGE,
    },
    write_stdout_marker,
};
use render::{ModelWitnessEvidence, ModelWitnessManifestRecord, TransparentWitnessEvidence};
use sha2::{Digest, Sha256};

/// Formats the fixed hash bytes used by witness identities.
fn lower_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Hashes the ordered model witness allocation manifest.
fn model_witness_manifest_hash(records: &[ModelWitnessManifestRecord]) -> String {
    let mut hasher = Sha256::new();
    for record in records {
        hasher.update(record.key.dimension.to_le_bytes());
        hasher.update(record.key.x.to_le_bytes());
        hasher.update(record.key.y.to_le_bytes());
        hasher.update(record.key.z.to_le_bytes());
        hasher.update(record.generation.to_le_bytes());
        hasher.update((record.model_ref_count as u64).to_le_bytes());
    }
    lower_hex(&hasher.finalize())
}

/// Emits the existing witness schema using the runtime's entity and visibility observations.
pub fn emit_witness_observations(
    transparent_witness: &TransparentWitnessEvidence,
    model_witness: &ModelWitnessEvidence,
    is_app_entity: impl Fn(world::SubChunkKey) -> bool,
    is_cave_visible: impl Fn(world::SubChunkKey) -> bool,
) {
    for event in transparent_witness.drain_events() {
        let marker = format!(
            "{TRANSPARENT_WITNESS_COMPLETE} revision={} sequence={} generation={} key_count={} consecutive={}",
            event.revision, event.sequence, event.generation, event.key_count, event.consecutive,
        );
        let mut stdout = diagnostics::console::stdout();
        write_stdout_marker(&mut stdout, &marker);
    }
    for event in model_witness.drain_events() {
        let acknowledgement = &event.acknowledgement;
        let marker = format!(
            "{MODEL_WITNESS_COMPLETE} revision={} request_sha256={} sequence={} view_generation={} key_count={} model_ref_count={} manifest_count={} manifest_sha256={} missing={} stale={} wrong_stream={} zero_ref={} draw_mismatch={} consecutive={}",
            acknowledgement.revision,
            lower_hex(&acknowledgement.request_hash),
            acknowledgement.frame_sequence,
            acknowledgement.view_generation,
            acknowledgement.manifest.len(),
            acknowledgement.total_model_ref_count,
            acknowledgement.manifest.len(),
            model_witness_manifest_hash(&acknowledgement.manifest),
            acknowledgement.missing_key_count,
            acknowledgement.stale_generation_count,
            acknowledgement.wrong_stream_count,
            acknowledgement.zero_model_ref_count,
            acknowledgement.draw_mismatch_count,
            event.consecutive,
        );
        let mut stdout = diagnostics::console::stdout();
        write_stdout_marker(&mut stdout, &marker);
    }
    for event in transparent_witness.drain_incomplete_events() {
        let missing = event
            .missing_keys
            .iter()
            .map(|key| format!("{},{},{},{}", key.dimension, key.x, key.y, key.z))
            .collect::<Vec<_>>()
            .join(";");
        let marker = format!(
            "{TRANSPARENT_WITNESS_INCOMPLETE} revision={} sequence={} generation={} missing_count={} missing={missing}",
            event.revision,
            event.sequence,
            event.generation,
            event.missing_keys.len(),
        );
        let mut stdout = diagnostics::console::stdout();
        write_stdout_marker(&mut stdout, &marker);
    }
    for event in transparent_witness.drain_stage_events() {
        let records = event
            .records
            .iter()
            .map(|record| {
                let app_entity = is_app_entity(record.key);
                format!(
                    "{},{},{},{}:app_entity={}:cave_visible={}:extracted_visible={}:instance={}:liquid_quads={}:instance_generation={}:allocation={}:liquid_range={}:lighting_range={}:allocation_matches={}:committed_member={}",
                    record.key.dimension,
                    record.key.x,
                    record.key.y,
                    record.key.z,
                    u8::from(app_entity),
                    u8::from(is_cave_visible(record.key)),
                    u8::from(record.extracted_visible),
                    u8::from(record.instance_present),
                    record.liquid_quad_count,
                    record.instance_generation,
                    u8::from(record.allocation_present),
                    record.liquid_range_len,
                    record.lighting_range_len,
                    u8::from(record.allocation_matches),
                    u8::from(record.committed_member),
                )
            })
            .collect::<Vec<_>>()
            .join(";");
        let marker = format!(
            "{TRANSPARENT_WITNESS_STAGE} revision={} committed_generation={} records={records}",
            event.revision, event.committed_generation,
        );
        let mut stdout = diagnostics::console::stdout();
        write_stdout_marker(&mut stdout, &marker);
    }
}
