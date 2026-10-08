use super::*;
use assets::EntityGeometryUv;

fn bounded_bone(name: &str, count: usize) -> EntityGeometryBone {
    let model = assets::parse_skin_geometry(
        r#"{"geometry":{"default":"geometry.fixture"}}"#,
        r#"{"format_version":"1.12.0","minecraft:geometry":[{
            "description":{"identifier":"geometry.fixture"},
            "bones":[{"name":"head","cubes":[{"origin":[0,0,0],"size":[1,1,1]}]}]
        }]}"#,
    )
    .unwrap()
    .unwrap();
    let mut bone = model.bones[0].clone();
    bone.name = name.into();
    bone.cubes = vec![bone.cubes[0].clone(); count].into_boxed_slice();
    bone
}

#[test]
fn entity_geometry_bone_budget_is_independent_of_player_skin_budget() {
    let bone_count = assets::MAX_SKIN_GEOMETRY_BONES + 1;
    let bones: Vec<_> = (0..bone_count)
        .map(|index| bounded_bone(&format!("part_{index}"), 0))
        .collect();
    let mut merged = Vec::new();
    let mut count = 0;
    append_geometry_bones(&mut merged, &mut count, &bones)
        .expect("entity rigs can exceed the separate player skin bone limit");
    let geometry = ActorRigGeometry::new(
        EntityRigId(1),
        vec![
            super::super::ActorRigVertex {
                bone_index: (bone_count - 1) as u32,
                ..Default::default()
            };
            3
        ],
        vec![[0.0; 3]; bone_count],
    )
    .unwrap();
    assert_eq!(merged.len(), bone_count);
    assert_eq!(geometry.bones_used(), bone_count);
}

#[test]
fn inherited_cube_budget_covers_all_bones_and_levels_before_append_or_clone() {
    let mut merged = Vec::new();
    let mut count = 0;
    let first_count = MAX_ENTITY_GEOMETRY_CUBES / 2;
    append_geometry_bones(
        &mut merged,
        &mut count,
        &[bounded_bone("head", first_count)],
    )
    .unwrap();
    append_geometry_bones(
        &mut merged,
        &mut count,
        &[bounded_bone(
            "body",
            MAX_ENTITY_GEOMETRY_CUBES - first_count,
        )],
    )
    .unwrap();
    let original_cubes = merged[0].cubes.as_ptr();
    for name in ["HEAD", "tail"] {
        assert_eq!(
            append_geometry_bones(&mut merged, &mut count, &[bounded_bone(name, 1)]),
            Err(ActorRigGeometryError::CatalogCapacity)
        );
        assert_eq!(merged.len(), 2);
        assert_eq!(count, MAX_ENTITY_GEOMETRY_CUBES);
        assert_eq!(merged[0].cubes.as_ptr(), original_cubes);
    }
}

#[test]
fn reset_releases_inherited_cube_budget_before_the_current_child_append() {
    let mut merged = Vec::new();
    let mut count = 0;
    append_geometry_bones(
        &mut merged,
        &mut count,
        &[bounded_bone("head", MAX_ENTITY_GEOMETRY_CUBES)],
    )
    .unwrap();
    let mut child = bounded_bone("head", 1);
    child.reset = Some(true);
    append_geometry_bones(&mut merged, &mut count, &[child]).unwrap();
    assert_eq!(count, 1);
    assert_eq!(merged[0].cubes.len(), 1);
    let remaining = MAX_ENTITY_GEOMETRY_CUBES - count;
    append_geometry_bones(&mut merged, &mut count, &[bounded_bone("body", remaining)]).unwrap();
    assert_eq!(count, MAX_ENTITY_GEOMETRY_CUBES);
    assert_eq!(
        append_geometry_bones(&mut merged, &mut count, &[bounded_bone("head", 1)]),
        Err(ActorRigGeometryError::CatalogCapacity),
        "the inherited reset flag cannot clear cubes on a later ordinary child"
    );
}

#[test]
fn inherited_head_face_and_outer_cube_both_emit_vertices() {
    let model = assets::parse_skin_geometry(
        r#"{"geometry":{"default":"geometry.coat"}}"#,
        r#"{"format_version":"1.8.0",
            "geometry.face":{"texturewidth":32,"textureheight":32,"bones":[{
                "name":"head","cubes":[{"origin":[-1,1,-3],"size":[2,2,3],"uv":[0,0]}]
            }]},
            "geometry.coat:geometry.face":{"bones":[{
                "name":"head","cubes":[{"origin":[-1,1,-1],"size":[2,2,2],"uv":[0,8]}]
            }]}}"#,
    )
    .unwrap()
    .unwrap();
    let mesh = skin_geometry(&model, EntityRigId(0)).unwrap();
    assert_eq!(
        mesh.vertices.len(),
        72,
        "face and coat are independent cubes"
    );
    assert!(
        mesh.vertices[..6]
            .iter()
            .all(|v| v.position[2] == -3.0 / 16.0)
    );
    assert!(
        mesh.vertices[36..42]
            .iter()
            .all(|v| v.position[2] == -1.0 / 16.0)
    );
    assert!(
        mesh.vertices[..6]
            .iter()
            .all(|v| v.uv[1] < mesh.vertices[36].uv[1])
    );
}

#[test]
#[ignore = "requires CINNABAR_ENTITY_CARRIER pointing to the pinned compiled entity catalog"]
fn pinned_adult_sheep_keeps_its_base_face_and_wool_overlay_baby_and_sheared_stay_single_cube() {
    let bytes = std::fs::read(std::env::var_os("CINNABAR_ENTITY_CARRIER").unwrap()).unwrap();
    let catalog = RuntimeEntityAssets::decode(&bytes).unwrap();
    let geometry = |name: &str| find_geometry_index(&catalog, name).unwrap() as usize;
    let adult_index = geometry("geometry.sheep.v1.8");
    let sheared_index = geometry("geometry.sheep.sheared.v1.8");
    let baby_index = geometry("geometry.sheep.baby");
    let head = |index| {
        resolve_geometry_bones(&catalog, index)
            .unwrap()
            .into_iter()
            .find(|bone| bone.name.as_ref() == "head")
            .unwrap()
    };
    let adult_head = head(adult_index);
    let sheared_head = head(sheared_index);
    let baby_head = head(baby_index);
    assert_eq!(adult_head.cubes.len(), 2);
    assert_eq!(sheared_head.cubes.len(), 1);
    assert_eq!(baby_head.cubes.len(), 1);
    assert_eq!(
        adult_head.cubes[0], sheared_head.cubes[0],
        "adult retains the real face geometry and UVs"
    );
    let asset = &catalog.geometries()[adult_index];
    let authored_head = asset
        .bones
        .iter()
        .find(|bone| bone.name.as_ref() == "head")
        .unwrap();
    assert_eq!(adult_head.cubes[1], authored_head.cubes[0]);
    let names = geometry_bone_names(&catalog, adult_index).unwrap();
    let head_index = names
        .iter()
        .position(|name| name.as_ref() == "head")
        .unwrap();
    let mesh = entity_geometry(&catalog, adult_index, EntityRigId(0)).unwrap();
    let vertices: Vec<_> = mesh
        .vertices
        .iter()
        .filter(|v| v.bone_index as usize == head_index)
        .collect();
    assert_eq!(vertices.len(), 72);
    let face = &sheared_head.cubes[0];
    let front_z = face.origin[2].get() / 16.0;
    assert!(vertices[..6].iter().all(|v| v.position[2] == front_z));
    let EntityGeometryUv::Box(uv) = &face.uv else {
        panic!("pinned face has a box atlas")
    };
    let left = (uv[0].get() + face.size[2].get()) / f32::from(asset.texture_width);
    let right = left + face.size[0].get() / f32::from(asset.texture_width);
    let top = (uv[1].get() + face.size[2].get()) / f32::from(asset.texture_height);
    let bottom = top + face.size[1].get() / f32::from(asset.texture_height);
    assert!(
        vertices[..6]
            .iter()
            .all(|v| (left..=right).contains(&v.uv[0]) && (top..=bottom).contains(&v.uv[1]))
    );
}
