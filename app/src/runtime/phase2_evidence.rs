//! Phase 2 evidence API consumed by the runtime observation adapter.
pub(crate) use acceptance::phase2_evidence::{
    CombinedPhase2Snapshot, PlayerColumnPresentationEvidence, build_profile_identity,
    generation_manifest_identity, graphics_identity_sha256, key_manifest_identity,
    phase2_publication_line_if_changed, phase2_publication_timing_line, present_mode_identity,
    sha256_identity_from_hex_or_text,
};
