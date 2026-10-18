use assets::{
    EntityDependencyResolution, EntityGeometry, EntityGeometryInheritance,
    EntityGeometryScalar as Scalar, EntityGeometryUv, neutral_actor_geometry_sampled_texels,
    neutral_actor_geometry_uvs_are_supported,
};

/// Builds a collapsed parent cube and a child that expands its inherited faces.
fn geometries(size: [f32; 3], uv: [f32; 2]) -> Vec<EntityGeometry> {
    let mut parent = super::geometry(
        size,
        EntityGeometryUv::Box(uv.map(|value| Scalar::new(value).unwrap())),
    );
    parent.texture_width = 16;
    parent.texture_height = 16;
    parent.bones[0].cubes[0].inflate = Scalar::new(-1.0).unwrap();
    let mut child = parent.clone();
    child.identifier = "geometry.expanded".into();
    child.inherits = Some(EntityGeometryInheritance {
        identifier: parent.identifier.clone(),
        resolution: EntityDependencyResolution::Catalog,
    });
    child.bones[0].cubes = Box::default();
    child.bones[0].inflate = Scalar::new(0.5);
    vec![parent, child]
}

#[test]
fn inherited_inflation_samples_faces_that_gained_area() {
    for (size, probe) in [
        ([2.0, 4.0, 4.0], [4, 0]),
        ([4.0, 2.0, 4.0], [0, 4]),
        ([4.0, 4.0, 2.0], [8, 2]),
    ] {
        let geometries = geometries(size, [0.0, 0.0]);
        for scale in [1, 2] {
            let width = 16 * scale;
            let texel = (probe[1] * scale * width + probe[0] * scale) as usize;
            let parent =
                neutral_actor_geometry_sampled_texels(&geometries, 0, width, width).unwrap();
            let child =
                neutral_actor_geometry_sampled_texels(&geometries, 1, width, width).unwrap();
            assert!(!parent[texel], "collapsed parent face: {size:?}");
            assert!(child[texel], "expanded inherited face: {size:?}");
        }
    }
}

#[test]
fn inherited_inflation_checks_uv_bounds_of_faces_that_gained_area() {
    let geometries = geometries([2.0, 4.0, 4.0], [0.0, -4.0]);
    assert!(neutral_actor_geometry_uvs_are_supported(&geometries, 0));
    assert!(!neutral_actor_geometry_uvs_are_supported(&geometries, 1));
    assert!(neutral_actor_geometry_sampled_texels(&geometries, 1, 16, 16).is_none());
}

#[test]
fn inherited_inflation_survives_sparse_and_explicit_zero_overlays() {
    let mut geometries = geometries([2.0, 4.0, 4.0], [0.0, 0.0]);
    for inflate in [None, Some(Scalar::ZERO), None] {
        let mut child = geometries.last().unwrap().clone();
        child.inherits.as_mut().unwrap().identifier = child.identifier.clone();
        child.identifier = format!("geometry.overlay{}", geometries.len()).into();
        child.bones[0].inflate = inflate;
        geometries.push(child);
    }
    for index in 1..geometries.len() {
        let sampled = neutral_actor_geometry_sampled_texels(&geometries, index, 16, 16).unwrap();
        assert!(sampled[4], "inherited face at level {index}");
    }
}

#[test]
fn inherited_inflation_leaves_new_cube_overrides_intact() {
    let mut geometries = geometries([2.0, 4.0, 4.0], [0.0, 0.0]);
    let authored = super::geometry(
        [0.0, 1.0, 3.0],
        EntityGeometryUv::Box([Scalar::new(2.0).unwrap(), Scalar::new(-3.0).unwrap()]),
    );
    geometries[1].bones[0].name = "BODY".into();
    geometries[1].bones[0].cubes = authored.bones[0].cubes.clone();
    assert!(neutral_actor_geometry_uvs_are_supported(&geometries, 1));
    let sampled = neutral_actor_geometry_sampled_texels(&geometries, 1, 16, 16).unwrap();
    assert!(sampled[8], "the inherited cube gains top and bottom faces");
}

#[test]
fn inherited_reset_removes_old_faces_from_the_sampled_texels() {
    let mut geometries = geometries([2.0, 4.0, 4.0], [0.0, 0.0]);
    geometries[1].bones[0].reset = Some(true);
    assert!(neutral_actor_geometry_uvs_are_supported(&geometries, 1));
    let sampled = neutral_actor_geometry_sampled_texels(&geometries, 1, 16, 16).unwrap();
    assert!(sampled.iter().all(|sampled| !sampled));
}
