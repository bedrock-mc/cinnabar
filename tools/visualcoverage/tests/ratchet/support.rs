use super::*;

pub(super) fn fixture_records() -> Vec<RegistryRecord> {
    let all = read_registry(include_bytes!(
        "../../../../crates/assets/data/block-registry-v1001.bin"
    ))
    .expect("read production registry");
    let mut air = all
        .iter()
        .find(|record| record.flags.contains(BlockFlags::AIR))
        .expect("air")
        .clone();
    let mut stone = all
        .iter()
        .find(|record| record.name.as_ref() == "minecraft:stone")
        .expect("stone")
        .clone();
    let mut vine = all
        .iter()
        .find(|record| {
            record.name.as_ref() == "minecraft:vine"
                && record.model_state.get(assets::ModelStateField::Connections) == Some(3)
        })
        .expect("vine mask 3")
        .clone();
    for (id, record) in [&mut air, &mut stone, &mut vine].into_iter().enumerate() {
        record.sequential_id = id as u32;
        record.network_hash = 0x8000_1000 + id as u32;
    }
    vec![air, stone, vine]
}

pub(super) fn texture_array(layers: u32) -> TextureArray {
    let mips = [16_u32, 8, 4, 2, 1]
        .into_iter()
        .map(|size| TextureMip {
            size,
            rgba8: vec![0x44; size as usize * size as usize * 4 * layers as usize]
                .into_boxed_slice(),
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    TextureArray { layers, mips }
}

pub(super) fn visual(kind: VisualKind) -> BlockVisual {
    BlockVisual {
        faces: match kind {
            VisualKind::Diagnostic | VisualKind::Invisible => [DIAGNOSTIC_MATERIAL; 6],
            _ => [1; 6],
        },
        flags: if kind == VisualKind::Cube {
            BlockFlags::CUBE_GEOMETRY
        } else {
            BlockFlags::empty()
        },
        kind,
        contributor_role: ContributorRole::Primary,
        model_template: NO_MODEL_TEMPLATE,
        animation: NO_ANIMATION,
        variant: 0,
    }
}

pub(super) fn blob(records: &[RegistryRecord], kinds: &[VisualKind]) -> Vec<u8> {
    let mut hashed = records
        .iter()
        .map(|record| (record.network_hash, record.sequential_id))
        .collect::<Vec<_>>();
    hashed.sort_unstable();
    let compiled = CompiledAssets {
        visuals: kinds.iter().copied().map(visual).collect(),
        light_properties: vec![assets::LightProperties::default(); kinds.len()].into_boxed_slice(),
        hashed: hashed.into_boxed_slice(),
        materials: vec![
            Material {
                texture: TextureRef::DIAGNOSTIC,
                flags: 0,
                animation: NO_ANIMATION,
            },
            Material {
                texture: TextureRef::new(0, 1).unwrap(),
                flags: 0,
                animation: NO_ANIMATION,
            },
        ]
        .into_boxed_slice(),
        model_templates: Box::new([]),
        model_quads: Box::new([]),
        animations: Box::new([]),
        animation_frames: Box::new([]),
        texture_pages: vec![TexturePage::new(texture_array(2))].into_boxed_slice(),
        biomes: CompiledBiomeAssets {
            tint_maps_rgb8: vec![0; TINT_MAP_BYTES].into_boxed_slice(),
            rules: vec![BiomeRule {
                id: 0,
                name: "minecraft:plains".into(),
                flags: 0,
                grass: TintSource::direct(0),
                foliage: TintSource::direct(0),
                dry_foliage: TintSource::direct(0),
                water: TintSource::direct(0),
                temperature_bits: 0,
                downfall_bits: 0,
            }]
            .into_boxed_slice(),
        },
    };
    encode_blob(&compiled).expect("encode fixture").into_vec()
}

pub(super) fn registry_bytes(records: &[RegistryRecord]) -> Vec<u8> {
    let mut bytes = b"BREG1003".to_vec();
    bytes.extend_from_slice(&1001_u32.to_le_bytes());
    let names = records
        .iter()
        .map(|record| record.name.as_ref())
        .collect::<std::collections::BTreeSet<_>>()
        .len() as u32;
    let valentine_names = records
        .iter()
        .filter(|record| record.provenance.contains(RegistryProvenance::VALENTINE))
        .map(|record| record.name.as_ref())
        .collect::<std::collections::BTreeSet<_>>()
        .len() as u32;
    let valentine_states = records
        .iter()
        .filter(|record| record.provenance.contains(RegistryProvenance::VALENTINE))
        .count() as u32;
    bytes.extend_from_slice(&names.to_le_bytes());
    bytes.extend_from_slice(&(records.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&valentine_names.to_le_bytes());
    bytes.extend_from_slice(&valentine_states.to_le_bytes());
    bytes.extend_from_slice(&(names - valentine_names).to_le_bytes());
    bytes.extend_from_slice(&((records.len() as u32) - valentine_states).to_le_bytes());
    for record in records {
        bytes.extend_from_slice(&record.sequential_id.to_le_bytes());
        bytes.extend_from_slice(&record.network_hash.to_le_bytes());
        bytes.push(record.flags.bits());
        bytes.push(record.model_family as u8);
        bytes.push(record.contributor_role as u8);
        bytes.push(record.model_state.mask());
        bytes.push(record.face_coverage);
        bytes.push(record.collision_seed.confidence as u8);
        bytes.push(record.provenance.bits());
        bytes.push(record.collision_seed.boxes.len() as u8);
        bytes.extend_from_slice(&record.collision_seed.shape_id.to_le_bytes());
        bytes.extend_from_slice(&(record.name.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&(record.canonical_state.len() as u32).to_le_bytes());
        for field in [
            assets::ModelStateField::Orientation,
            assets::ModelStateField::Half,
            assets::ModelStateField::Open,
            assets::ModelStateField::Hinge,
            assets::ModelStateField::Connections,
            assets::ModelStateField::Growth,
            assets::ModelStateField::LiquidDepth,
            assets::ModelStateField::Flags,
        ] {
            bytes.extend_from_slice(&record.model_state.get(field).unwrap_or(0).to_le_bytes());
        }
        for collision_box in &record.collision_seed.boxes {
            for value in [
                collision_box.min_x,
                collision_box.min_y,
                collision_box.min_z,
                collision_box.max_x,
                collision_box.max_y,
                collision_box.max_z,
            ] {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        bytes.extend_from_slice(record.name.as_bytes());
        bytes.extend_from_slice(record.canonical_state.as_bytes());
    }
    bytes
}

pub(super) fn baseline(report: &visualcoverage::CoverageSnapshot) -> Baseline {
    Baseline {
        schema: "cinnabar-visual-coverage-baseline-v1".into(),
        protocol: 1001,
        registry_sha256: report.registry_sha256.clone(),
        counts: report.counts,
        states: report.states.clone(),
        diagnostic_sequential_ids: report
            .diagnostic_states
            .iter()
            .map(|state| state.sequential_id)
            .collect(),
        invisible_allowlist: Vec::new(),
        expected_vine_diagnostic_masks: vec![3],
    }
}

pub(super) fn strict_fixture_records(families: &[ModelFamily]) -> Vec<RegistryRecord> {
    let all = read_registry(include_bytes!(
        "../../../../crates/assets/data/block-registry-v1001.bin"
    ))
    .expect("read production registry");
    families
        .iter()
        .enumerate()
        .map(|(index, &family)| {
            let mut record = all
                .iter()
                .find(|record| record.model_family == family)
                .unwrap_or_else(|| panic!("missing fixture record for {family:?}"))
                .clone();
            record.sequential_id = index as u32;
            record.network_hash = 0x9100_0000 + index as u32;
            record
        })
        .collect()
}

pub(super) fn strict_no_draw(flags: BlockFlags, role: ContributorRole) -> BlockVisual {
    BlockVisual {
        faces: [DIAGNOSTIC_MATERIAL; 6],
        flags,
        kind: VisualKind::Invisible,
        contributor_role: role,
        model_template: NO_MODEL_TEMPLATE,
        animation: NO_ANIMATION,
        variant: 0,
    }
}

pub(super) fn strict_cube(faces: [u32; 6]) -> BlockVisual {
    BlockVisual {
        faces,
        flags: BlockFlags::CUBE_GEOMETRY,
        kind: VisualKind::Cube,
        contributor_role: ContributorRole::Primary,
        model_template: NO_MODEL_TEMPLATE,
        animation: NO_ANIMATION,
        variant: 0,
    }
}

pub(super) fn strict_model(kind: VisualKind, template: u32) -> BlockVisual {
    BlockVisual {
        faces: [DIAGNOSTIC_MATERIAL; 6],
        flags: BlockFlags::empty(),
        kind,
        contributor_role: ContributorRole::Primary,
        model_template: template,
        animation: NO_ANIMATION,
        variant: 0,
    }
}

pub(super) fn strict_liquid(faces: [u32; 6], variant: u32) -> BlockVisual {
    BlockVisual {
        faces,
        flags: BlockFlags::empty(),
        kind: VisualKind::Liquid,
        contributor_role: ContributorRole::LiquidAdditional,
        model_template: NO_MODEL_TEMPLATE,
        animation: NO_ANIMATION,
        variant,
    }
}

pub(super) fn strict_diagnostic(flags: BlockFlags, role: ContributorRole) -> BlockVisual {
    BlockVisual {
        faces: [DIAGNOSTIC_MATERIAL; 6],
        flags,
        kind: VisualKind::Diagnostic,
        contributor_role: role,
        model_template: NO_MODEL_TEMPLATE,
        animation: NO_ANIMATION,
        variant: 0,
    }
}

pub(super) fn strict_quad(material: u32) -> ModelQuad {
    ModelQuad {
        positions: [[0, 0, 0], [256, 0, 0], [256, 256, 0], [0, 256, 0]],
        uvs: [[0, 0], [4096, 0], [4096, 4096], [0, 4096]],
        material,
        flags: 0,
    }
}

pub(super) fn strict_runtime(
    records: &[RegistryRecord],
    visuals: Vec<BlockVisual>,
    materials: Vec<Material>,
    templates: Vec<ModelTemplate>,
    quads: Vec<ModelQuad>,
    animations: Vec<Animation>,
    frames: Vec<TextureRef>,
) -> RuntimeAssets {
    RuntimeAssets::decode(&strict_blob(
        records, visuals, materials, templates, quads, animations, frames,
    ))
    .expect("decode strict fixture")
}

pub(super) fn strict_blob(
    records: &[RegistryRecord],
    visuals: Vec<BlockVisual>,
    mut materials: Vec<Material>,
    templates: Vec<ModelTemplate>,
    quads: Vec<ModelQuad>,
    animations: Vec<Animation>,
    frames: Vec<TextureRef>,
) -> Vec<u8> {
    if animations.is_empty() {
        for material in &mut materials {
            material.animation = NO_ANIMATION;
        }
    }
    let mut hashed = records
        .iter()
        .map(|record| (record.network_hash, record.sequential_id))
        .collect::<Vec<_>>();
    hashed.sort_unstable();
    let compiled = CompiledAssets {
        visuals: visuals.into_boxed_slice(),
        light_properties: vec![assets::LightProperties::default(); records.len()]
            .into_boxed_slice(),
        hashed: hashed.into_boxed_slice(),
        materials: materials.into_boxed_slice(),
        model_templates: templates.into_boxed_slice(),
        model_quads: quads.into_boxed_slice(),
        animations: animations.into_boxed_slice(),
        animation_frames: frames.into_boxed_slice(),
        texture_pages: vec![TexturePage::new(texture_array(8))].into_boxed_slice(),
        biomes: CompiledBiomeAssets {
            tint_maps_rgb8: vec![0; TINT_MAP_BYTES].into_boxed_slice(),
            rules: vec![BiomeRule {
                id: 0,
                name: "minecraft:plains".into(),
                flags: 0,
                grass: TintSource::direct(0),
                foliage: TintSource::direct(0),
                dry_foliage: TintSource::direct(0),
                water: TintSource::direct(0),
                temperature_bits: 0,
                downfall_bits: 0,
            }]
            .into_boxed_slice(),
        },
    };
    encode_blob(&compiled)
        .expect("encode strict fixture")
        .into_vec()
}

pub(super) fn strict_materials() -> Vec<Material> {
    vec![
        Material {
            texture: TextureRef::DIAGNOSTIC,
            flags: 0,
            animation: NO_ANIMATION,
        },
        Material {
            texture: TextureRef::new(0, 1).unwrap(),
            flags: 0,
            animation: NO_ANIMATION,
        },
        Material {
            texture: TextureRef::new(0, 2).unwrap(),
            flags: 0,
            animation: 0,
        },
        Material {
            texture: TextureRef::new(0, 3).unwrap(),
            flags: MATERIAL_FLAG_ALPHA_BLEND | MATERIAL_FLAG_WATER_TINT,
            animation: 0,
        },
        Material {
            texture: TextureRef::new(0, 4).unwrap(),
            flags: MATERIAL_FLAG_LIQUID_DEPTH_WRITE,
            animation: 0,
        },
    ]
}

pub(super) fn strict_animations() -> (Vec<Animation>, Vec<TextureRef>) {
    (
        vec![Animation {
            frame_start: 0,
            frame_count: 2,
            ticks_per_frame: 1,
            atlas_index: 0,
            atlas_tile_variant: 0,
            replicate: 1,
            flags: ANIMATION_FLAG_BLEND,
        }],
        vec![
            TextureRef::new(0, 5).unwrap(),
            TextureRef::new(0, 6).unwrap(),
        ],
    )
}

pub(super) fn strict_baseline(
    snapshot: &visualcoverage::CoverageSnapshot,
    invisible: &[StateIdentity],
) -> Baseline {
    Baseline {
        schema: visualcoverage::BASELINE_SCHEMA.into(),
        protocol: 1001,
        registry_sha256: snapshot.registry_sha256.clone(),
        counts: snapshot.counts,
        states: snapshot.states.clone(),
        diagnostic_sequential_ids: snapshot
            .diagnostic_states
            .iter()
            .map(|state| state.sequential_id)
            .collect(),
        invisible_allowlist: invisible
            .iter()
            .cloned()
            .map(|state| AllowlistEntry {
                state,
                authority: "Reviewed no-draw fixture".into(),
                source: "https://example.invalid/strict-fixture".into(),
            })
            .collect(),
        expected_vine_diagnostic_masks: snapshot.vine_diagnostic_masks.clone(),
    }
}

pub(super) fn strict_snapshot(
    records: &[RegistryRecord],
    runtime: &RuntimeAssets,
) -> visualcoverage::CoverageSnapshot {
    analyze_records(records, runtime, "registry-hash", "assets-hash")
        .expect("analyze strict fixture")
}

pub(super) fn assert_no_atomic_temp_artifacts(directory: &std::path::Path) {
    let artifacts = std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains(".tmp-"))
        .collect::<Vec<_>>();
    assert!(
        artifacts.is_empty(),
        "orphaned atomic temp files: {artifacts:?}"
    );
}

pub(super) struct SerializationFailure;

impl Serialize for SerializationFailure {
    fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        Err(serde::ser::Error::custom(
            "intentional serialization failure",
        ))
    }
}

pub(super) fn strict_stair_templates() -> Vec<ModelTemplate> {
    (0..5)
        .map(|quad_start| ModelTemplate {
            quad_start,
            quad_count: 1,
            flags: MODEL_TEMPLATE_FLAG_STAIR,
        })
        .collect()
}
