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
        material: None,
        params: mod_api::materials::Preset::default().0,
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

#[test]
fn materials_require_authority_and_share_transaction_and_handle_lifetimes() {
    let mut store = Store::default();
    let mut denied = DrawingPermissions::ALL;
    denied.render_material = false;
    let source = crate::material::BUILTIN;
    assert_eq!(
        store.create_material(source, denied).unwrap_err().code,
        ErrorCode::Denied
    );
    let stale = store
        .create_material(source, DrawingPermissions::ALL)
        .unwrap();
    store.rollback();
    store.begin();
    let material = store
        .create_material(source, DrawingPermissions::ALL)
        .unwrap();
    let geometry = store.create_geometry(mesh()).unwrap();
    let mut instance = drawing(geometry);
    instance.material = Some(stale);
    assert_eq!(
        store
            .create(instance.clone(), DrawingPermissions::ALL)
            .unwrap_err()
            .code,
        ErrorCode::StaleHandle
    );
    instance.material = Some(material);
    assert_eq!(
        store.create(instance.clone(), denied).unwrap_err().code,
        ErrorCode::Denied
    );
    let id = store
        .create(instance.clone(), DrawingPermissions::ALL)
        .unwrap();
    store.commit();
    assert_eq!(store.remove(material).unwrap_err().code, ErrorCode::InUse);
    let snapshot = store.snapshot().clone();
    store.begin();
    instance.params = mod_api::materials::Preset::rainbow(2.).0;
    store.update(id, instance, DrawingPermissions::ALL).unwrap();
    store.commit();
    assert!(Arc::ptr_eq(
        &snapshot.geometry[&geometry],
        &store.snapshot().geometry[&geometry]
    ));
    store.remove(id).unwrap();
    store.remove(material).unwrap();
    store.commit();
    assert!(store.snapshot().materials.is_empty());
    store.revoke();
    assert_eq!(
        store.remove(material).unwrap_err().code,
        ErrorCode::StaleHandle
    );
}

#[test]
fn rejected_compiles_and_retiring_materials_consume_their_budgets() {
    let mut store = Store::default();
    assert_eq!(
        store
            .create_material("bad source", DrawingPermissions::ALL)
            .unwrap_err()
            .code,
        ErrorCode::Invalid
    );
    assert_eq!(
        store
            .create_material(crate::material::BUILTIN, DrawingPermissions::ALL)
            .unwrap_err()
            .code,
        ErrorCode::BudgetExceeded
    );
    let mut handles = Vec::new();
    for _ in 0..mod_api::MAX_DRAW_MATERIALS {
        store.begin();
        handles.push(
            store
                .create_material(crate::material::BUILTIN, DrawingPermissions::ALL)
                .unwrap(),
        );
        store.commit();
    }
    let snapshot = store.snapshot().clone();
    store.begin();
    store.remove(handles[0]).unwrap();
    store.commit();
    store.begin();
    assert_eq!(
        store
            .create_material(crate::material::BUILTIN, DrawingPermissions::ALL)
            .unwrap_err()
            .code,
        ErrorCode::BudgetExceeded
    );
    drop(snapshot);
    store.commit();
    store.begin();
    store
        .create_material(crate::material::BUILTIN, DrawingPermissions::ALL)
        .unwrap();
}
