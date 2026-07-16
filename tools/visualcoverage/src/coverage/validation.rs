use super::*;

pub(super) fn validate_baseline(baseline: &Baseline) -> Result<(), CoverageError> {
    if baseline.schema != BASELINE_SCHEMA || baseline.protocol != PROTOCOL {
        return Err(CoverageError::UnsupportedBaseline);
    }
    if !is_strictly_sorted_u32(&baseline.diagnostic_sequential_ids)
        || !is_strictly_sorted_by_id(&baseline.states)
        || !is_strictly_sorted_u8(&baseline.expected_vine_diagnostic_masks)
        || !baseline
            .diagnostic_sequential_ids
            .iter()
            .all(|&sequential_id| (sequential_id as usize) < baseline.states.len())
        || !baseline
            .invisible_allowlist
            .windows(2)
            .all(|pair| pair[0].state < pair[1].state)
    {
        return Err(CoverageError::NonCanonicalBaseline);
    }
    Ok(())
}

pub(super) fn validate_protocol_snapshot(snapshot: &CoverageSnapshot) -> Result<(), CoverageError> {
    if snapshot.protocol != PROTOCOL || snapshot.counts != PROTOCOL_1001_COUNTS {
        return Err(CoverageError::NonCanonicalProtocolInventory(
            "snapshot counts",
        ));
    }
    validate_protocol_states(&snapshot.states)
}

pub(super) fn validate_protocol_baseline(baseline: &Baseline) -> Result<(), CoverageError> {
    if baseline.protocol != PROTOCOL || baseline.counts != PROTOCOL_1001_COUNTS {
        return Err(CoverageError::NonCanonicalProtocolInventory(
            "baseline counts",
        ));
    }
    validate_protocol_states(&baseline.states)
}

pub(super) fn validate_protocol_states(states: &[StateIdentity]) -> Result<(), CoverageError> {
    if states.len() != PROTOCOL_1001_COUNTS.states {
        return Err(CoverageError::NonCanonicalProtocolInventory(
            "state vector length",
        ));
    }
    let names = states
        .iter()
        .map(|state| state.name.as_str())
        .collect::<BTreeSet<_>>()
        .len();
    if names != PROTOCOL_1001_COUNTS.names {
        return Err(CoverageError::NonCanonicalProtocolInventory(
            "state name cardinality",
        ));
    }
    if states.iter().filter(|state| state.is_air).count() != PROTOCOL_1001_COUNTS.air {
        return Err(CoverageError::NonCanonicalProtocolInventory(
            "air cardinality",
        ));
    }
    Ok(())
}

pub(super) fn is_strictly_sorted_by_id(values: &[StateIdentity]) -> bool {
    values
        .iter()
        .enumerate()
        .all(|(index, state)| state.sequential_id == index as u32)
}

pub(super) fn is_strictly_sorted_u8(values: &[u8]) -> bool {
    values.windows(2).all(|pair| pair[0] < pair[1])
}

pub(super) fn is_strictly_sorted_u32(values: &[u32]) -> bool {
    values.windows(2).all(|pair| pair[0] < pair[1])
}

pub(super) fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(super) fn visual_kind_name(kind: VisualKind) -> &'static str {
    match kind {
        VisualKind::Diagnostic => "diagnostic",
        VisualKind::Cube => "cube",
        VisualKind::Cross => "cross",
        VisualKind::Model => "model",
        VisualKind::Liquid => "liquid",
        VisualKind::Invisible => "invisible",
    }
}

pub(super) fn model_family_name(family: ModelFamily) -> &'static str {
    match family {
        ModelFamily::Unknown => "unknown",
        ModelFamily::Air => "air",
        ModelFamily::Cube => "cube",
        ModelFamily::Leaves => "leaves",
        ModelFamily::Cross => "cross",
        ModelFamily::Crop => "crop",
        ModelFamily::Liquid => "liquid",
        ModelFamily::Slab => "slab",
        ModelFamily::Stair => "stair",
        ModelFamily::Door => "door",
        ModelFamily::Trapdoor => "trapdoor",
        ModelFamily::Pane => "pane",
        ModelFamily::Fence => "fence",
        ModelFamily::Gate => "gate",
        ModelFamily::Chest => "chest",
        ModelFamily::Sign => "sign",
        ModelFamily::Wall => "wall",
        ModelFamily::Bed => "bed",
        ModelFamily::Rail => "rail",
        ModelFamily::Torch => "torch",
        ModelFamily::Button => "button",
        ModelFamily::PressurePlate => "pressure_plate",
        ModelFamily::Carpet => "carpet",
        ModelFamily::Layer => "layer",
        ModelFamily::Decorative => "decorative",
        ModelFamily::Statue => "statue",
        ModelFamily::Cuboid => "cuboid",
        ModelFamily::Aquatic => "aquatic",
        ModelFamily::Cocoa => "cocoa",
        ModelFamily::Lever => "lever",
        ModelFamily::Invisible => "invisible",
        ModelFamily::FlowerBed => "flower_bed",
        ModelFamily::Vine => "vine",
        ModelFamily::GlowLichen => "glow_lichen",
        ModelFamily::SculkVein => "sculk_vein",
        ModelFamily::ChiseledBookshelf => "chiseled_bookshelf",
        ModelFamily::ResinClump => "resin_clump",
    }
}
