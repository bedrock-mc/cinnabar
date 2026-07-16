mod coverage;

pub use coverage::{
    AllowlistEntry, BASELINE_SCHEMA, Baseline, Counts, CoverageError, CoverageSnapshot,
    GALLERY_INVENTORY_SCHEMA, GALLERY_PAGE_CAPACITY, GalleryInventory, GalleryPage, GalleryTarget,
    GalleryTargetStatus, InvisibleDecision, MAX_BASELINE_BYTES, PROTOCOL, PROTOCOL_1001_COUNTS,
    REPORT_SCHEMA, RatchetReport, RenderStream, STRICT_REPORT_SCHEMA, StateIdentity, StrictReport,
    StrictStateRoute, analyze_bytes, analyze_records, baseline_from_snapshot, deterministic_json,
    gallery_inventory_bytes, parse_baseline, ratchet, ratchet_protocol_1001, strict_bytes,
    strict_records, write_deterministic_json_atomic,
};
