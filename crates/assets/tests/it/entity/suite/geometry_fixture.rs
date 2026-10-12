use super::*;

/// Build a finite scalar for the synthetic geometry.
pub(super) fn scalar(value: f32) -> EntityGeometryScalar {
    EntityGeometryScalar::new(value).expect("finite bounded geometry scalar")
}

/// Build a catalog and bounded geometry for carrier validation tests.
pub(super) fn geometry_fixture() -> CompiledEntityAssets {
    let geometry_source = br#"{"format_version":"1.21.0"}"#;
    let mut compiled = fixture();
    compiled.sources = vec![
        compiled.sources[0].clone(),
        EntityAssetSource {
            path: "models/entity/allay.geo.json".into(),
            source_bytes: geometry_source.len() as u32,
            source_sha256: Sha256::digest(geometry_source).into(),
        },
    ]
    .into_boxed_slice();
    compiled.symbols[0].dependencies[0].resolution = EntityDependencyResolution::Catalog;
    compiled.symbols = vec![
        compiled.symbols[0].clone(),
        EntityAssetSymbol {
            mount_equipment: assets::entity::mount::MountEquipment::Unresolved,
            kind: EntityAssetKind::Geometry,
            identifier: "geometry.allay".into(),
            source_index: 1,
            dependencies: Box::new([]),
        },
    ]
    .into_boxed_slice();
    compiled.geometries = vec![EntityGeometry {
        visible_bounds: None,
        identifier: "geometry.allay".into(),
        inherits: None,
        source_index: 1,
        texture_width: 32,
        texture_height: 32,
        bones: vec![EntityGeometryBone {
            name: "wing".into(),
            parent: None,
            pivot: Some([scalar(0.5), scalar(4.0), scalar(1.0)]),
            rotation: Some([scalar(0.0), scalar(15.0), scalar(0.0)]),
            bind_pose_rotation: None,
            mirror: None,
            inflate: None,
            never_render: None,
            reset: None,
            binding: None,
            texture_meshes: Box::new([]),
            cubes: vec![EntityGeometryCube {
                origin: [scalar(0.5), scalar(-1.0), scalar(1.0)],
                size: [scalar(0.0), scalar(5.0), scalar(8.0)],
                pivot: [scalar(0.5), scalar(4.0), scalar(1.0)],
                rotation: [scalar(0.0), scalar(0.0), scalar(-2.5)],
                uv: EntityGeometryUv::Box([scalar(16.0), scalar(14.0)]),
                inflate: scalar(-0.2),
                mirror: true,
            }]
            .into_boxed_slice(),
        }]
        .into_boxed_slice(),
    }]
    .into_boxed_slice();
    compiled
}
