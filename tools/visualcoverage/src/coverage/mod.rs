use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use assets::{
    BlockFace, BlockFlags, ContributorRole, DIAGNOSTIC_MATERIAL, MATERIAL_FLAG_ALPHA_BLEND,
    MATERIAL_FLAG_LIQUID_DEPTH_WRITE, MATERIAL_FLAG_WATER_TINT, MODEL_TEMPLATE_FLAG_COMPOUND_NEXT,
    MODEL_TEMPLATE_FLAG_FENCE_NETHER, MODEL_TEMPLATE_FLAG_FENCE_WOOD, MODEL_TEMPLATE_FLAG_PANE,
    MODEL_TEMPLATE_FLAG_STAIR, ModelFamily, ModelStateField, NetworkIdMode, RegistryRecord,
    RuntimeAssets, TextureRef, VisualKind, read_registry,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

mod report_io;
mod snapshot;
mod strict;
mod types;
mod validation;

pub use report_io::{deterministic_json, parse_baseline, write_deterministic_json_atomic};
pub use snapshot::{
    analyze_bytes, analyze_records, baseline_from_snapshot, ratchet, ratchet_protocol_1001,
};
pub use strict::{gallery_inventory_bytes, strict_bytes, strict_records};
pub use types::*;

use validation::{
    model_family_name, sha256, validate_baseline, validate_protocol_baseline,
    validate_protocol_snapshot, visual_kind_name,
};
