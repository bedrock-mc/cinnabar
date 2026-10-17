use super::*;
use assets::block_entity_geometry::heads::HeadModel;

const SOURCE: &str = include_str!("fixtures/cube_defaults.geo.json");

/// Compiles only original geometry into a normally admitted catalog, without local game assets.
fn catalog() -> RuntimeEntityAssets {
    catalog_from_source(SOURCE)
}

/// Compiles one original geometry document with the required empty source families.
fn catalog_from_source(source: &str) -> RuntimeEntityAssets {
    let scratch = tempfile::tempdir().unwrap();
    for family in [
        "entity",
        "models/entity",
        "animations",
        "animation_controllers",
        "render_controllers",
        "textures/entity",
    ] {
        std::fs::create_dir_all(scratch.path().join(family)).unwrap();
    }
    std::fs::write(
        scratch.path().join("models/entity/defaults.geo.json"),
        source,
    )
    .unwrap();
    let compiled = pack_entity_compiler::compile_entity_assets(
        scratch.path(),
        include_bytes!("../../../../assets/vanilla-source.json"),
    )
    .unwrap();
    RuntimeEntityAssets::from_compiled(compiled).unwrap()
}

/// Checks actual rendered extents in authored pixel units, after the renderer's X reflection.
fn assert_bounds(vertices: &[super::super::ActorRigVertex], min: [f32; 3], max: [f32; 3]) {
    for axis in 0..3 {
        let low = vertices
            .iter()
            .map(|vertex| vertex.position[axis] * 16.0)
            .fold(f32::INFINITY, f32::min);
        let high = vertices
            .iter()
            .map(|vertex| vertex.position[axis] * 16.0)
            .fold(f32::NEG_INFINITY, f32::max);
        assert!(
            (low - min[axis]).abs() < 1e-5,
            "axis {axis}: {low} != {}",
            min[axis]
        );
        assert!(
            (high - max[axis]).abs() < 1e-5,
            "axis {axis}: {high} != {}",
            max[axis]
        );
    }
}

#[test]
fn compiled_and_skin_cubes_share_resolved_defaults_overrides_and_inheritance() {
    let catalog = catalog();
    for (name, count) in [
        ("geometry.defaults", 2),
        ("geometry.derived_defaults", 3),
        ("geometry.sparse_defaults", 2),
    ] {
        let index = find_geometry_index(&catalog, name).unwrap() as usize;
        let mesh = entity_geometry(&catalog, index, EntityRigId(0)).unwrap();
        let patch = serde_json::json!({"geometry":{"default":name}}).to_string();
        let skin = assets::parse_skin_geometry(&patch, SOURCE)
            .unwrap()
            .unwrap();
        let skin_mesh = skin_geometry(&skin, EntityRigId(0)).unwrap();
        assert_eq!(
            mesh.vertices, skin_mesh.vertices,
            "bounds, face orientation and UVs: {name}"
        );
        let texture = assets::EquipmentTexture {
            identifier: "textures/test/defaults".into(),
            width: 16,
            height: 16,
            rgba8: vec![255; 16 * 16 * 4].into(),
        };
        let equipment = super::super::texture_mesh::attachable_geometry(
            &catalog,
            index,
            EntityRigId(0),
            &texture,
        )
        .unwrap();
        assert_eq!(
            equipment.vertices, mesh.vertices,
            "attachable cube path: {name}"
        );
        assert_eq!(mesh.vertices.len(), count * 36);
        let inherited_inflate = if count == 3 { 0.5 } else { 0.25 };
        assert_bounds(
            &mesh.vertices[..36],
            [
                -2.0 - inherited_inflate,
                1.0 - inherited_inflate,
                -inherited_inflate,
            ],
            [
                inherited_inflate,
                3.0 + inherited_inflate,
                2.0 + inherited_inflate,
            ],
        );
        let second_inflate = if name == "geometry.defaults" {
            0.0
        } else {
            inherited_inflate
        };
        assert_bounds(
            &mesh.vertices[36..72],
            [-6.0 - second_inflate, 1.0 - second_inflate, -second_inflate],
            [
                -4.0 + second_inflate,
                3.0 + second_inflate,
                2.0 + second_inflate,
            ],
        );

        let bones = resolve_geometry_bones(&catalog, index).unwrap();
        let head = bones
            .iter()
            .find(|bone| bone.name.eq_ignore_ascii_case("head"))
            .unwrap();
        assert_eq!(
            head.inflate.unwrap().get(),
            if count == 3 { 0.5 } else { 0.25 }
        );
        assert_eq!(head.mirror, Some(count == 2));
        let mut geometry = catalog.geometries()[index].clone();
        geometry.bones = bones.into();
        let head_model = HeadModel::build_tree(&geometry, Some("head"), 1.0, false).unwrap();
        assert_eq!(head_model.boxes.len(), count);
        assert_eq!(head_model.boxes[0].spec.inflate, inherited_inflate);
        assert_eq!(head_model.boxes[1].spec.inflate, second_inflate);
        if count == 3 {
            assert_bounds(&mesh.vertices[72..], [-10.5, 0.5, -0.5], [-7.5, 3.5, 2.5]);
            assert_eq!(head_model.boxes[2].spec.inflate, 0.5);
        }
    }
}

#[test]
fn eligibility_uses_the_same_explicit_zero_inflate_as_mesh_generation() {
    let catalog = catalog();
    let index = find_geometry_index(&catalog, "geometry.plane_defaults").unwrap() as usize;
    assert!(assets::neutral_actor_geometry_uvs_are_supported(
        catalog.geometries(),
        index
    ));
    assert_eq!(
        assets::neutral_actor_geometry_sampled_texels(catalog.geometries(), index, 2, 2),
        Some(vec![true; 4])
    );
    let mesh = entity_geometry(&catalog, index, EntityRigId(0)).unwrap();
    assert_eq!(mesh.vertices.len(), 12);
    assert!(mesh.vertices.iter().all(|vertex| vertex.position[2] == 0.0));
}

#[test]
fn compiled_fin_defaults_and_overrides_validate_every_face_that_gained_area() {
    let catalog = catalog();
    for (name, inflate, supported, vertices) in [
        ("geometry.fin_bone_default", 0.1, false, 36),
        ("geometry.fin_cube_inflate", 0.1, false, 36),
        ("geometry.fin_explicit_zero", 0.0, true, 12),
    ] {
        let index = find_geometry_index(&catalog, name).unwrap() as usize;
        let geometry = &catalog.geometries()[index];
        assert_eq!(geometry.bones[0].cubes[0].inflate.get(), inflate, "{name}");
        assert_eq!(
            assets::neutral_actor_geometry_uvs_are_supported(catalog.geometries(), index),
            supported,
            "UVs on the resolved cube: {name}"
        );
        let mesh = entity_geometry(&catalog, index, EntityRigId(0)).unwrap();
        assert_eq!(mesh.vertices.len(), vertices, "drawable faces: {name}");
        let patch = serde_json::json!({"geometry":{"default":name}}).to_string();
        let skin = assets::parse_skin_geometry(&patch, SOURCE)
            .unwrap()
            .unwrap();
        assert_eq!(
            mesh.vertices,
            skin_geometry(&skin, EntityRigId(0)).unwrap().vertices
        );
    }
}

/// An armour shell inherits its cubes before a derived bone expands them around the skin.
#[test]
fn inherited_armour_inflation_keeps_the_shell_outside_the_player_body() {
    let source = r#"{"format_version":"1.8.0",
        "geometry.armour_base":{"texturewidth":32,"textureheight":32,"bones":[{
            "name":"body","pivot":[0,24,0],
            "cubes":[{"origin":[-4,12,-2],"size":[8,12,4],"uv":[0,0]}]}]},
        "geometry.armour_shell:geometry.armour_base":{"bones":[{"name":"body","inflate":1.0}]}
    }"#;
    let catalog = catalog_from_source(source);
    let index = find_geometry_index(&catalog, "geometry.armour_shell").unwrap() as usize;
    let mesh = entity_geometry(&catalog, index, EntityRigId(0)).unwrap();
    assert_bounds(&mesh.vertices, [-5.0, 11.0, -3.0], [5.0, 25.0, 3.0]);
}
