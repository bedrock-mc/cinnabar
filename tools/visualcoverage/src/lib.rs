//! Frozen protocol-1001 visual-coverage evidence replay.
//!
//! This crate is deliberately historical. Active protocol selection belongs to
//! `assets/bedrock-target.json`; runtime, packaging, and acceptance code must not
//! use this crate to choose a protocol or asset carrier.

mod coverage;

pub use coverage::{
    AllowlistEntry, BASELINE_SCHEMA, Baseline, Counts, CoverageError, CoverageSnapshot,
    GALLERY_INVENTORY_SCHEMA, GALLERY_PAGE_CAPACITY, GalleryInventory, GalleryPage, GalleryTarget,
    GalleryTargetStatus, InvisibleDecision, MAX_BASELINE_BYTES, PROTOCOL, PROTOCOL_1001_COUNTS,
    PUBLIC_TARGET_COUNT, REPORT_SCHEMA, RatchetReport, RenderStream, STRICT_REPORT_SCHEMA,
    StateIdentity, StrictReport, StrictStateRoute, analyze_bytes, analyze_records,
    baseline_from_snapshot, deterministic_json, gallery_inventory_bytes, parse_baseline, ratchet,
    ratchet_protocol_1001, strict_bytes, strict_records, write_deterministic_json_atomic,
};
