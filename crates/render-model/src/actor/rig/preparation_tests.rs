use super::*;

thread_local! {
    static VALIDATED_VERTICES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Measures vertex validation work on the test thread without elapsed-time assertions.
pub(super) fn record_validation(vertices: usize) {
    VALIDATED_VERTICES.with(|count| count.set(count.get() + vertices));
}

/// Creates a valid mesh whose public allocations can be replaced after preparation.
fn prepared_mesh() -> ActorRigGeometry {
    ActorRigGeometry::synthetic_cuboid(EntityRigId(1), [0.0; 3], [1.0; 3], 2).unwrap()
}

/// Admission into body and hand catalogs must reuse work performed by the mesh owner.
#[test]
fn prepared_geometry_reuses_validation_for_unchanged_allocations() {
    let prepared = prepared_mesh();
    VALIDATED_VERTICES.with(|count| count.set(0));
    for id in [EntityRigId(2), EntityRigId(3)] {
        let mut geometry = prepared.clone();
        geometry.id = id;
        geometry.revalidate().unwrap();
        assert_eq!(geometry.bones_used(), prepared.bones_used());
    }
    assert_eq!(VALIDATED_VERTICES.with(std::cell::Cell::get), 0);
}

/// Public mutation must not inherit the validation result of the original allocation.
#[test]
fn changed_geometry_allocations_are_revalidated() {
    let original = prepared_mesh();
    let mut changed = original.clone();
    Arc::make_mut(&mut changed.vertices)[0].position[0] = f32::NAN;
    assert_eq!(
        changed.revalidate(),
        Err(ActorRigGeometryError::InvalidVertex)
    );
    let mut changed = original.clone();
    Arc::make_mut(&mut changed.bone_pivots)[0][1] = f32::INFINITY;
    assert_eq!(
        changed.revalidate(),
        Err(ActorRigGeometryError::InvalidVertex)
    );
    let mut changed = original;
    Arc::make_mut(&mut changed.vertices)[0].bone_index = 1;
    changed.revalidate().unwrap();
    assert_eq!(changed.bones_used(), 2);
}

/// Equal content has equal fingerprints, while changed allocations cannot use stale metadata.
#[test]
fn prepared_fingerprints_track_exact_vertex_allocations() {
    let mut mesh = prepared_mesh();
    let same =
        ActorRigGeometry::new(mesh.id, mesh.vertices.to_vec(), mesh.bone_pivots.to_vec()).unwrap();
    let fingerprint = mesh.vertex_fingerprint().unwrap();
    assert_eq!(same.vertex_fingerprint(), Some(fingerprint));
    assert!(Arc::get_mut(&mut mesh.vertices).is_none());
    assert!(Arc::get_mut(&mut mesh.bone_pivots).is_none());
    Arc::make_mut(&mut mesh.vertices)[0].uv[0] = -0.0;
    assert_eq!(mesh.vertex_fingerprint(), None);
    mesh.revalidate().unwrap();
    assert_ne!(mesh.vertex_fingerprint(), Some(fingerprint));
}
