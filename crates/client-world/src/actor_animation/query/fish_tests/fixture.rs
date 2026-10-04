use super::*;
use assets::*;

fn scalar(value: f32) -> EntityGeometryScalar {
    EntityGeometryScalar::new(value).unwrap()
}

/// Two synthetic geometries let lifetime tests exercise the real selection/reset path.
pub(super) fn assets(identifier: &str) -> Arc<RuntimeEntityAssets> {
    let sources = [
        "entity/fish.entity.json",
        "models/entity/fish.geo.json",
        "render_controllers/fish.render_controllers.json",
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
        (EntityAssetKind::Entity, identifier, 0),
        (EntityAssetKind::Geometry, "geometry.fish.a", 1),
        (EntityAssetKind::Geometry, "geometry.fish.b", 1),
        (
            EntityAssetKind::RenderController,
            "controller.render.fish",
            2,
        ),
    ]
    .into_iter()
    .map(|(kind, identifier, source_index)| EntityAssetSymbol {
        kind,
        identifier: identifier.into(),
        source_index,
        dependencies: Box::new([]),
    })
    .collect::<Vec<_>>()
    .into_boxed_slice();
    let geometries = ["geometry.fish.a", "geometry.fish.b"]
        .into_iter()
        .map(|identifier| EntityGeometry {
            identifier: identifier.into(),
            inherits: None,
            source_index: 1,
            texture_width: 16,
            texture_height: 16,
            bones: vec![EntityGeometryBone {
                name: "body".into(),
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
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let molang_symbols = [
        (MolangSymbolKind::Query, "query.variant"),
        (MolangSymbolKind::Variable, "variable.animationamount"),
        (MolangSymbolKind::Variable, "variable.animationamountprev"),
    ]
    .into_iter()
    .map(|(kind, identifier)| MolangSymbol {
        kind,
        identifier: identifier.into(),
    })
    .collect::<Vec<_>>()
    .into_boxed_slice();
    let compiled = CompiledEntityAssets {
        source_manifest_sha256: [1; 32],
        block_visual_count: 1,
        sources,
        symbols,
        geometries,
        animation_clips: Box::new([]),
        animation_channels: Box::new([]),
        animation_keyframes: Box::new([]),
        molang_symbols,
        molang_expressions: vec![CompiledMolangExpression {
            first_op: 0,
            op_count: 2,
            max_stack: 1,
        }]
        .into_boxed_slice(),
        molang_ops: vec![MolangOp::LoadQuery(0), MolangOp::Truthy].into_boxed_slice(),
        molang_collections: Box::new([]),
        molang_collection_items: Box::new([]),
        controllers: Box::new([]),
        controller_states: Box::new([]),
        controller_animations: Box::new([]),
        controller_transitions: Box::new([]),
        rig_bindings: vec![EntityRigBinding {
            entity_symbol: 0,
            render_controller: 3,
            first_geometry: 0,
            geometry_count: 2,
            fallback: EntityRigFallback::Skip,
            initialize: None,
            pre_animation: None,
            scale: scalar(1.0),
            scale_expressions: None,
        }]
        .into_boxed_slice(),
        rig_geometries: [None, Some(0)]
            .into_iter()
            .enumerate()
            .map(|(geometry, condition)| EntityRigGeometryBinding {
                geometry: geometry as u32,
                condition,
                first_animation: 0,
                animation_count: 0,
                first_controller: 0,
                controller_count: 0,
            })
            .collect::<Vec<_>>()
            .into_boxed_slice(),
        rig_animations: Box::new([]),
        rig_controllers: Box::new([]),
        item_visuals: Box::new([]),
        item_visual_aliases: Box::new([]),
        render: EntityRenderData::default(),
    };
    Arc::new(RuntimeEntityAssets::from_compiled(compiled).unwrap())
}
