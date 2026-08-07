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
    let mut reserved = 0;
    for state in states {
        let selected = is_reserved_sequential_id(state.sequential_id);
        if selected {
            reserved += 1;
            if state.name != "cinnabar:reserved"
                || state.canonical_state
                    != format!(
                        "{{\"reserved_id\":{{\"type\":\"int\",\"value\":{}}}}}",
                        state.sequential_id
                    )
                || state.model_family != "unknown"
                || state.is_air
            {
                return Err(CoverageError::NonCanonicalProtocolInventory(
                    "reserved state identity",
                ));
            }
        } else if state.name == "cinnabar:reserved" {
            return Err(CoverageError::NonCanonicalProtocolInventory(
                "reserved state selection",
            ));
        }
    }
    if reserved != 383 {
        return Err(CoverageError::NonCanonicalProtocolInventory(
            "reserved state cardinality",
        ));
    }
    Ok(())
}

pub(super) fn validate_protocol_records(records: &[RegistryRecord]) -> Result<(), CoverageError> {
    let states = records
        .iter()
        .map(StateIdentity::from_record)
        .collect::<Vec<_>>();
    validate_protocol_states(&states)?;
    for record in records {
        if !is_reserved_sequential_id(record.sequential_id) {
            continue;
        }
        if !record.flags.is_empty()
            || record.model_family != ModelFamily::Unknown
            || record.contributor_role != ContributorRole::Primary
            || record.model_state != Default::default()
            || record.face_coverage != 0
            || record.collision_seed.shape_id != 0
            || record.collision_seed.confidence != CollisionConfidence::None
            || !record.collision_seed.boxes.is_empty()
        {
            return Err(CoverageError::NonCanonicalProtocolInventory(
                "reserved state semantics",
            ));
        }
    }
    Ok(())
}

pub(super) fn is_reserved_sequential_id(id: u32) -> bool {
    matches!(id,
        1 | 382 | 1258 | 1266 | 1273 | 1528 | 2712 | 3339..=3340 | 3778 | 3784 |
        4152..=4157 | 5373..=5388 | 5526..=5529 | 6105 | 6316 | 6416 |
        6770..=6775 | 6847 | 6858..=6859 | 7295 | 7300..=7303 | 7306 | 7332 |
        7362..=7365 | 7989 | 8713..=8874 | 9257 | 10018..=10036 | 10394 | 11699 |
        12519 | 12726 | 12827 | 12891 | 12944 | 12953..=12958 | 13526 | 13819 |
        13839 | 13967 | 14573 | 14586 | 14637..=14646 | 14648 | 15027 | 15115 |
        15231..=15320 | 15374..=15379 | 15867 | 16121..=16124 | 16834..=16839 |
        16842)
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
