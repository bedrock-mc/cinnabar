//! Asset and skin fixtures shared by animation tests and actor authority's integration tests.
use super::*;
use assets::*;
use render_api::SkinGeometrySource;

fn scalar(value: f32) -> EntityGeometryScalar {
    EntityGeometryScalar::new(value).unwrap()
}

/// A minimal compiled catalog with one item attachable rig.
pub fn compiled_fixture() -> CompiledEntityAssets {
    let sources = [
        "animations/item.json",
        "attachables/item.json",
        "models/entity/item.json",
        "render_controllers/item.json",
        "textures/items/item.png",
    ]
    .into_iter()
    .map(|path| EntityAssetSource {
        path: path.into(),
        source_bytes: 1,
        source_sha256: [1; 32],
    })
    .collect::<Vec<_>>()
    .into_boxed_slice();
    let symbols = [
        (EntityAssetKind::Geometry, "geometry.item", 2),
        (EntityAssetKind::Animation, "animation.item", 0),
        (
            EntityAssetKind::RenderController,
            "controller.render.item",
            3,
        ),
        (EntityAssetKind::Texture, "textures/items/item", 4),
        (EntityAssetKind::Attachable, "minecraft:test_item", 1),
    ]
    .into_iter()
    .map(|(kind, identifier, source_index)| EntityAssetSymbol {
        mount_equipment: assets::entity::mount::MountEquipment::Unresolved,
        kind,
        identifier: identifier.into(),
        source_index,
        dependencies: Box::new([]),
    })
    .collect::<Vec<_>>()
    .into_boxed_slice();
    let molang_symbols = [
        (MolangSymbolKind::Name, "wield"),
        (MolangSymbolKind::Query, "query.frame_alpha"),
        (MolangSymbolKind::Query, "query.get_animation_frame"),
        (MolangSymbolKind::Query, "query.main_hand_item_max_duration"),
        (MolangSymbolKind::Query, "query.main_hand_item_use_duration"),
        (MolangSymbolKind::Variable, "context.is_first_person"),
        (MolangSymbolKind::Variable, "context.is_paperdoll"),
        (MolangSymbolKind::Variable, "variable.charge_amount"),
    ]
    .into_iter()
    .map(|(kind, identifier)| MolangSymbol {
        kind,
        identifier: identifier.into(),
    })
    .collect::<Vec<_>>()
    .into_boxed_slice();
    CompiledEntityAssets {
        source_manifest_sha256: [1; 32],
        block_visual_count: 1,
        sources,
        symbols,
        geometries: vec![EntityGeometry {
            visible_bounds: None,
            identifier: "geometry.item".into(),
            inherits: None,
            source_index: 2,
            texture_width: 16,
            texture_height: 16,
            bones: vec![EntityGeometryBone {
                name: "rightitem".into(),
                binding: None,
                parent: None,
                pivot: Some([scalar(0.0); 3]),
                rotation: None,
                bind_pose_rotation: None,
                mirror: None,
                inflate: None,
                never_render: None,
                reset: None,
                cubes: Box::new([]),
                texture_meshes: Box::new([]),
            }]
            .into_boxed_slice(),
        }]
        .into_boxed_slice(),
        animation_clips: vec![EntityAnimationClip {
            symbol: 1,
            length_seconds: scalar(0.0),
            loop_mode: EntityAnimationLoop::Loop,
            first_channel: 0,
            channel_count: 1,
            source: 0,
            override_previous: false,
            anim_time_update: None,
            geometry: Some(0),
        }]
        .into_boxed_slice(),
        animation_channels: vec![EntityAnimationChannel {
            bone_name: None,
            bone: 0,
            property: EntityAnimationProperty::Translation,
            first_keyframe: 0,
            keyframe_count: 1,
            rotation_relative_to_entity: false,
        }]
        .into_boxed_slice(),
        animation_keyframes: vec![EntityAnimationKeyframe {
            time_seconds: scalar(0.0),
            value: [scalar(0.0); 3],
            interpolation: EntityAnimationInterpolation::Linear,
            expressions: [Some(1), Some(2), None],
        }]
        .into_boxed_slice(),
        molang_symbols,
        molang_expressions: [(0, 11, 3), (11, 1, 1), (12, 1, 1), (13, 1, 1)]
            .into_iter()
            .map(|(first_op, op_count, max_stack)| CompiledMolangExpression {
                first_op,
                op_count,
                max_stack,
            })
            .collect::<Vec<_>>()
            .into_boxed_slice(),
        // Native attachable charge formula, expressed through the shared compiled VM.
        molang_ops: vec![
            MolangOp::LoadQuery(3),
            MolangOp::LoadQuery(4),
            MolangOp::LoadQuery(1),
            MolangOp::Subtract,
            MolangOp::Push(scalar(1.0)),
            MolangOp::Add,
            MolangOp::Subtract,
            MolangOp::Push(scalar(10.0)),
            MolangOp::Divide,
            MolangOp::StoreVariable(7),
            MolangOp::Push(scalar(0.0)),
            MolangOp::LoadVariable(5),
            MolangOp::LoadVariable(7),
            MolangOp::LoadQuery(2),
        ]
        .into_boxed_slice(),
        molang_collections: Box::new([]),
        molang_collection_items: Box::new([]),
        controllers: Box::new([]),
        controller_states: Box::new([]),
        controller_animations: Box::new([]),
        controller_transitions: Box::new([]),
        rig_bindings: vec![EntityRigBinding {
            entity_symbol: 4,
            render_controller: 2,
            first_geometry: 0,
            geometry_count: 1,
            fallback: EntityRigFallback::Skip,
            initialize: None,
            pre_animation: Some(0),
            scale: scalar(1.0),
            scale_expressions: None,
        }]
        .into_boxed_slice(),
        rig_geometries: vec![EntityRigGeometryBinding {
            geometry: 0,
            condition: None,
            first_animation: 0,
            animation_count: 1,
            first_controller: 0,
            controller_count: 0,
        }]
        .into_boxed_slice(),
        rig_animations: vec![EntityRigAnimationBinding {
            name: 0,
            clip: 0,
            weight: None,
            order: 0,
        }]
        .into_boxed_slice(),
        rig_controllers: Box::new([]),
        item_visuals: Box::new([]),
        item_visual_aliases: Box::new([]),
        render: EntityRenderData {
            layers: vec![EntityRenderLayer {
                material: Default::default(),
                material_state: None,
                hurt_color: None,
                rig: 0,
                condition: None,
                first_slot: 0,
                slot_count: 1,
                first_visibility: 0,
                visibility_count: 0,
                color: None,
                overlay_color: None,
                on_fire_color: None,
                uv_anim: None,
                first_geometry: 0,
                geometry_count: 0,
                ignore_lighting: false,
                light_color_multiplier: None,
            }]
            .into_boxed_slice(),
            slots: vec![EntityRenderSlot {
                first_candidate: 0,
                candidate_count: 1,
            }]
            .into_boxed_slice(),
            candidates: vec![EntityRenderCandidate {
                condition: None,
                source: 4,
            }]
            .into_boxed_slice(),
            ..EntityRenderData::default()
        },
    }
}

/// A `minecraft:test` rig whose pre-animation counts ticks and draws a random number, posing both.
pub fn counting_random_assets() -> Arc<RuntimeEntityAssets> {
    counting_random_assets_for("minecraft:test")
}

/// Gives actor fixtures the same deterministic rig under their actual entity identifier.
pub fn counting_random_assets_for(identifier: &str) -> Arc<RuntimeEntityAssets> {
    Arc::new(RuntimeEntityAssets::from_compiled(counting_random_compiled(identifier)).unwrap())
}

/// Builds a mutable counting rig for tests that add render-controller branches.
pub fn counting_random_compiled(identifier: &str) -> assets::CompiledEntityAssets {
    let mut compiled = compiled_fixture();
    compiled.sources[1].path = "entity/item.json".into();
    compiled.symbols[4].kind = EntityAssetKind::Entity;
    compiled.symbols[4].identifier = identifier.into();
    compiled.symbols.rotate_right(1);
    compiled.rig_bindings[0].entity_symbol = 0;
    compiled.rig_bindings[0].render_controller = 3;
    compiled.animation_clips[0].symbol = 2;
    let scalar = |value| EntityGeometryScalar::new(value).unwrap();
    compiled.molang_ops = vec![
        MolangOp::LoadVariable(7),
        MolangOp::Push(scalar(1.0)),
        MolangOp::Add,
        MolangOp::StoreVariable(7),
        MolangOp::Push(scalar(0.0)),
        MolangOp::Push(scalar(1.0)),
        MolangOp::Call(MolangFunction::Random),
        MolangOp::StoreVariable(5),
        MolangOp::Push(scalar(0.0)),
        MolangOp::LoadVariable(7),
        MolangOp::LoadVariable(5),
        MolangOp::LoadQuery(1),
    ]
    .into_boxed_slice();
    compiled.molang_expressions = [(0, 9, 2), (9, 1, 1), (10, 1, 1), (11, 1, 1)]
        .into_iter()
        .map(|(first_op, op_count, max_stack)| CompiledMolangExpression {
            first_op,
            op_count,
            max_stack,
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    compiled.render.layers[0].color = Some([1, 2, 3, 3]);
    compiled
}

const SKIN_PATCH: &str = r#"{"geometry":{"default":"geometry.skin_cache"}}"#;
/// A one-cube custom skin model.
pub const SKIN_MODEL: &str = r#"{"format_version":"1.12.0","minecraft:geometry":[{
"description":{"identifier":"geometry.skin_cache","texture_width":64,"texture_height":64},
"bones":[{"name":"root","pivot":[0,0,0]},{"name":"body","parent":"root","pivot":[0,12,0],
"cubes":[{"origin":[-4,0,-2],"size":[8,12,4],"uv":[0,0]}]}]}]}"#;

/// Builds independent source allocations, as separate player-list entries arrive from the wire.
pub fn skin_source(model: &str) -> Arc<SkinGeometrySource> {
    Arc::new(SkinGeometrySource {
        resource_patch: SKIN_PATCH.to_string().into(),
        geometry_data: model.to_string().into(),
        animations: Arc::from([]),
    })
}

/// A custom entity with initialized pose inputs and metadata-controlled part visibility.
pub fn initialized_pose_compiled() -> assets::CompiledEntityAssets {
    let mut compiled = counting_random_compiled("minecraft:test");
    let mut symbols = compiled.molang_symbols.into_vec();
    let query = 5;
    symbols.insert(
        query as usize,
        MolangSymbol {
            kind: MolangSymbolKind::Query,
            identifier: "query.show_bottom".into(),
        },
    );
    compiled.molang_symbols = symbols.into_boxed_slice();
    let mut ops = compiled.molang_ops.into_vec();
    for op in &mut ops {
        if let MolangOp::LoadVariable(index) | MolangOp::StoreVariable(index) = op {
            *index += 1;
        }
    }
    let mut expressions = compiled.molang_expressions.into_vec();
    let visibility = expressions.len() as u32;
    expressions.push(CompiledMolangExpression {
        first_op: ops.len() as u32,
        op_count: 1,
        max_stack: 1,
    });
    ops.push(MolangOp::LoadQuery(query));
    compiled.render.visibility = vec![EntityRenderVisibility {
        pattern: "rightitem".into(),
        condition: visibility,
    }]
    .into_boxed_slice();
    compiled.render.layers[0].visibility_count = 1;
    compiled.animation_keyframes[0].expressions = [None, Some(1), None];
    compiled.rig_bindings[0].initialize = Some(expressions.len() as u32);
    expressions.push(CompiledMolangExpression {
        first_op: ops.len() as u32,
        op_count: 3,
        max_stack: 1,
    });
    ops.extend([
        MolangOp::Push(EntityGeometryScalar::new(5.0).unwrap()),
        MolangOp::StoreVariable(8),
        MolangOp::Push(EntityGeometryScalar::new(0.0).unwrap()),
    ]);
    compiled.molang_ops = ops.into_boxed_slice();
    compiled.molang_expressions = expressions.into_boxed_slice();
    compiled
}

/// The installed entity carrier, or `None` after naming the missing `fixture`.
pub fn installed_entity_assets(fixture: &str) -> Option<Arc<RuntimeEntityAssets>> {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/assets/compiled");
    let entries = match std::fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!(
                "skipping installed {fixture} fixture: {} is absent",
                root.display()
            );
            return None;
        }
        Err(error) => panic!("read {}: {error}", root.display()),
    };
    let paths = entries
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "mcbeent")
        })
        .collect::<Vec<_>>();
    if paths.is_empty() {
        eprintln!(
            "skipping installed {fixture} fixture: no entity carrier in {}",
            root.display()
        );
        return None;
    }
    assert_eq!(
        paths.len(),
        1,
        "ambiguous installed entity carriers: {paths:?}"
    );
    Some(Arc::new(
        RuntimeEntityAssets::decode(&std::fs::read(&paths[0]).unwrap()).unwrap(),
    ))
}

/// The bone's per-axis scale with its uniform scale applied.
pub fn total_scale(transform: &super::BoneTransform) -> [f32; 3] {
    super::pose::total_scale(transform)
}

/// The quaternion `[x, y, z, w]` for authored Euler degrees.
pub fn quat_from_euler(rotation: [f32; 3]) -> [f32; 4] {
    super::pose::quat_from_euler(rotation)
}

/// Skin preparations the calling thread has run, so tests can prove the frame thread does none.
pub fn skin_preparations_on_this_thread() -> usize {
    super::skin::prepared_on_this_thread()
}

pub const WOLF_IDENTIFIER: &str = super::query::wolf::WOLF_IDENTIFIER;
/// The metadata key whose byte value selects a tamed wolf's dye colour.
pub const WOLF_COLOR_METADATA_KEY: u32 = super::query::wolf::COLOR_METADATA_KEY;

/// The metadata key whose short value selects a thrown potion's appearance.
pub const POTION_AUX_VALUE_METADATA_KEY: u32 = super::query::potion::AUX_VALUE_METADATA_KEY;

pub const MAX_SKIN_BATCHES_IN_FLIGHT: usize = super::skin::queue::MAX_SKIN_BATCHES_IN_FLIGHT;
pub const MAX_SKIN_PREPARATIONS_PER_PASS: usize =
    super::skin::queue::MAX_SKIN_PREPARATIONS_PER_PASS;

impl ActorAnimationStore {
    /// Caps each tick's world-wide expression budget, to exercise starvation and resumption.
    pub fn set_world_budget(&mut self, budget: usize) {
        self.schedule.world_budget = budget;
    }
}
