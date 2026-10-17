use super::*;

/// Makes a reusable unit triangle with distinct vertex colours.
fn mesh() -> GeometrySpec {
    GeometrySpec::Triangles {
        vertices: [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]]
            .map(|position| Vertex {
                position,
                color: [1.; 4],
                uv: [0.; 2],
                data: [0.; 4],
            })
            .to_vec(),
        indices: vec![0, 1, 2],
    }
}
/// Makes an identity instance referencing the caller's admitted geometry.
fn drawing(geometry: Handle) -> Instance {
    Instance {
        geometry,
        anchor: Anchor::World {
            dimension: 0,
            origin: [0.; 3],
        },
        transform: Transform::default(),
        tint: [1.; 4],
        depth: Depth::Test,
        blend: Blend::Premultiplied,
        width: 1.,
        width_mode: WidthMode::World,
        join: Join::Bevel,
        cap: Cap::Butt,
    }
}

#[test]
fn rollback_and_revoke_never_recycle_tokens_or_publish_partial_output() {
    let mut store = Store::default();
    let stale = store.create_geometry(mesh()).unwrap();
    store.rollback();
    let geometry = store.create_geometry(mesh()).unwrap();
    assert_ne!(geometry, stale);
    assert_eq!(
        store
            .create(drawing(stale), DrawingPermissions::ALL)
            .unwrap_err()
            .code,
        ErrorCode::StaleHandle
    );
    let id = store
        .create(drawing(geometry), DrawingPermissions::ALL)
        .unwrap();
    store.commit();
    assert_eq!(store.snapshot().instances.len(), 1);
    store.revoke();
    assert!(store.snapshot().geometry.is_empty());
    assert_eq!(
        store
            .update(id, drawing(geometry), DrawingPermissions::ALL)
            .unwrap_err()
            .code,
        ErrorCode::StaleHandle
    );
}

#[test]
fn instances_share_meshes_and_motion_changes_only_instance_revision() {
    let mut store = Store::default();
    let geometry = store.create_geometry(mesh()).unwrap();
    let id = store
        .create(drawing(geometry), DrawingPermissions::ALL)
        .unwrap();
    store
        .create(drawing(geometry), DrawingPermissions::ALL)
        .unwrap();
    store.commit();
    let before = Arc::clone(store.snapshot());
    store.begin();
    let mut moved = drawing(geometry);
    moved.transform.translation[0] = 2.;
    store.update(id, moved, DrawingPermissions::ALL).unwrap();
    store.commit();
    assert!(Arc::ptr_eq(
        &before.geometry[&geometry],
        &store.snapshot().geometry[&geometry]
    ));
    assert_eq!(before.geometry_revision, store.snapshot().geometry_revision);
    assert_ne!(before.instance_revision, store.snapshot().instance_revision);
    store.begin();
    store.commit();
    let unchanged = Arc::clone(store.snapshot());
    store.begin();
    store.commit();
    assert!(Arc::ptr_eq(&unchanged, store.snapshot()));
}

#[test]
fn foreign_handles_and_referenced_geometry_cannot_be_removed() {
    let mut first = Store::default();
    let mut second = Store::default();
    let geometry = first.create_geometry(mesh()).unwrap();
    assert_eq!(
        second
            .create(drawing(geometry), DrawingPermissions::ALL)
            .unwrap_err()
            .code,
        ErrorCode::StaleHandle
    );
    let id = first
        .create(drawing(geometry), DrawingPermissions::ALL)
        .unwrap();
    assert_eq!(first.remove(geometry).unwrap_err().code, ErrorCode::InUse);
    first.remove(id).unwrap();
    first.remove(geometry).unwrap();
    first.commit();
    assert!(first.snapshot().geometry.is_empty() && first.snapshot().instances.is_empty());
}

#[test]
fn invalid_geometry_and_through_wall_draws_leave_the_transaction_unchanged() {
    let mut store = Store::default();
    let GeometrySpec::Triangles { vertices, .. } = mesh() else {
        unreachable!()
    };
    assert_eq!(
        store
            .create_geometry(GeometrySpec::Triangles {
                vertices,
                indices: vec![0, 1, 3]
            })
            .unwrap_err()
            .code,
        ErrorCode::Invalid
    );
    let geometry = store.create_geometry(mesh()).unwrap();
    let mut instance = drawing(geometry);
    instance.depth = Depth::Always;
    assert_eq!(
        store
            .create(
                instance,
                DrawingPermissions {
                    world_draw: true,
                    ..Default::default()
                }
            )
            .unwrap_err()
            .code,
        ErrorCode::Denied
    );
    store.commit();
    assert!(store.snapshot().instances.is_empty());
}

#[test]
fn frame_groups_expire_only_after_success_and_are_atomic_on_failure() {
    let mut store = Store::default();
    let geometry = store.create_geometry(mesh()).unwrap();
    store
        .replace_group(
            "cursor".into(),
            vec![drawing(geometry)],
            DrawingPermissions::ALL,
        )
        .unwrap();
    store.commit();
    let before = Arc::clone(store.snapshot());
    store.begin();
    store.rollback();
    assert!(Arc::ptr_eq(&before, store.snapshot()));
    store.begin();
    let mut invalid = drawing(geometry);
    invalid.width = f32::NAN;
    assert!(
        store
            .replace_group(
                "cursor".into(),
                vec![drawing(geometry), invalid],
                DrawingPermissions::ALL
            )
            .is_err()
    );
    store.commit();
    assert!(store.snapshot().instances.is_empty());
}

#[test]
fn polylines_have_host_computed_arc_length_and_bounded_callback_payloads() {
    let mut store = Store::default();
    let GeometrySpec::Triangles { mut vertices, .. } = mesh() else {
        unreachable!()
    };
    vertices.truncate(2);
    vertices[1].position = [3., 4., 0.];
    let line = store
        .create_geometry(GeometrySpec::Polyline {
            vertices,
            closed: false,
        })
        .unwrap();
    store.commit();
    assert_eq!(&*store.snapshot().geometry[&line].distances, &[0., 5.]);
    store.begin();
    let vertex = store.snapshot().geometry[&line].vertices[0];
    let error = store
        .create_geometry(GeometrySpec::Polyline {
            vertices: vec![vertex; mod_api::MAX_DRAW_VERTICES],
            closed: false,
        })
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::BudgetExceeded);
    store.commit();
    assert_eq!(store.snapshot().geometry.len(), 1);
}

#[test]
fn resident_and_retiring_geometry_share_the_same_quota() {
    let mut store = Store::default();
    let ids = (0..mod_api::MAX_DRAW_GEOMETRIES)
        .map(|_| store.create_geometry(mesh()).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        store.create_geometry(mesh()).unwrap_err().detail,
        "geometries"
    );
    store.commit();
    let held = Arc::clone(store.snapshot());
    store.begin();
    for id in ids {
        store.remove(id).unwrap();
    }
    assert_eq!(
        store.create_geometry(mesh()).unwrap_err().detail,
        "geometries"
    );
    store.commit();
    store.begin();
    drop(held);
    store.create_geometry(mesh()).unwrap();
    store.commit();
    assert_eq!(store.snapshot().geometry.len(), 1);
}

#[test]
fn instance_overflow_has_a_typed_error_and_does_not_truncate() {
    let mut store = Store::default();
    let geometry = store.create_geometry(mesh()).unwrap();
    for _ in 0..mod_api::MAX_DRAW_INSTANCES {
        store
            .create(drawing(geometry), DrawingPermissions::ALL)
            .unwrap();
    }
    assert_eq!(
        store
            .create(drawing(geometry), DrawingPermissions::ALL)
            .unwrap_err()
            .detail,
        "instances"
    );
    store.commit();
    assert_eq!(
        store.snapshot().instances.len(),
        mod_api::MAX_DRAW_INSTANCES
    );
}

#[test]
fn triangle_expansion_preserves_indices_and_zero_length_lines_emit_nothing() {
    let mut store = Store::default();
    let triangle = store.create_geometry(mesh()).unwrap();
    let vertex = Vertex {
        position: [0.; 3],
        color: [1.; 4],
        uv: [0.; 2],
        data: [0.; 4],
    };
    let line = store
        .create_geometry(GeometrySpec::Polyline {
            vertices: vec![vertex; 3],
            closed: false,
        })
        .unwrap();
    store.commit();
    let expanded = super::mesh::build(&store.snapshot().geometry[&triangle]);
    assert_eq!(expanded.len(), 3);
    assert_eq!(expanded[1].a[..3], [1., 0., 0.]);
    assert!(super::mesh::build(&store.snapshot().geometry[&line]).is_empty());
}

/// Names a session-bound actor incarnation for lifetime probes.
fn actor(spawn_revision: u64) -> EntityKey {
    EntityKey {
        session: 1,
        dimension: 0,
        runtime_id: 7,
        spawn_revision,
    }
}

#[test]
fn absent_candidates_preserve_instances_but_confirmed_despawn_removes_them() {
    let mut store = Store::default();
    store.bind_session(1, 0);
    store.reconcile_entities(vec![actor(1)], &[]).unwrap();
    let target = store.observe_entities().unwrap()[0].0;
    let geometry = store.create_geometry(mesh()).unwrap();
    let mut instance = drawing(geometry);
    instance.anchor = Anchor::Entity(EntityAnchor {
        entity: target,
        body_rotation: false,
    });
    store
        .create(instance.clone(), DrawingPermissions::ALL)
        .unwrap();
    store.commit();
    store.reconcile_entities(Vec::new(), &[]).unwrap();
    assert_eq!(store.snapshot().instances.len(), 1);
    store
        .reconcile_entities(vec![actor(2)], &[actor(1)])
        .unwrap();
    assert!(store.snapshot().instances.is_empty());
    assert_eq!(
        store
            .create(instance, DrawingPermissions::ALL)
            .unwrap_err()
            .code,
        ErrorCode::StaleHandle
    );
    let replacement = store.observe_entities().unwrap()[0].0;
    assert_ne!(target, replacement);
}

#[test]
fn entity_references_are_owner_scoped_and_cannot_be_freed_while_attached() {
    let mut first = Store::default();
    let mut second = Store::default();
    first.reconcile_entities(vec![actor(1)], &[]).unwrap();
    let target = first.observe_entities().unwrap()[0].0;
    let geometry = second.create_geometry(mesh()).unwrap();
    let mut instance = drawing(geometry);
    instance.anchor = Anchor::Entity(EntityAnchor {
        entity: target,
        body_rotation: true,
    });
    assert_eq!(
        second
            .create(instance, DrawingPermissions::ALL)
            .unwrap_err()
            .code,
        ErrorCode::StaleHandle
    );
    let geometry = first.create_geometry(mesh()).unwrap();
    let mut instance = drawing(geometry);
    instance.anchor = Anchor::Entity(EntityAnchor {
        entity: target,
        body_rotation: false,
    });
    first.create(instance, DrawingPermissions::ALL).unwrap();
    assert_eq!(first.remove(target).unwrap_err().code, ErrorCode::InUse);
}

#[test]
fn entity_reference_capacity_errors_preserve_the_previous_selection() {
    let mut store = Store::default();
    let first: Vec<_> = (0..mod_api::MAX_DRAW_ENTITIES)
        .map(|index| EntityKey {
            runtime_id: index as u64 + 1,
            ..actor(1)
        })
        .collect();
    store.reconcile_entities(first, &[]).unwrap();
    store.observe_entities().unwrap();
    store.commit();
    let previous = Arc::clone(store.snapshot());
    store.begin();
    store.reconcile_entities(vec![actor(2)], &[]).unwrap();
    assert_eq!(
        store.observe_entities().unwrap_err().detail,
        "entity handles"
    );
    store.rollback();
    assert!(Arc::ptr_eq(&previous, store.snapshot()));
}
