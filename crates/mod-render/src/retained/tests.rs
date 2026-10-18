use super::*;

mod sdk;
pub(super) mod work;

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
    let material = store
        .create_material(crate::material::BUILTIN, DrawingPermissions::ALL)
        .unwrap();
    let mut instance = drawing(geometry);
    instance.material = Some(material);
    instance.params = mod_api::materials::Preset::rainbow(2.).0;
    instance.anchor = Anchor::Entity(EntityAnchor {
        entity: target,
        body_rotation: false,
    });
    store
        .create(instance.clone(), DrawingPermissions::ALL)
        .unwrap();
    store.commit();
    let before = Arc::clone(store.snapshot());
    store.reconcile_entities(Vec::new(), &[]).unwrap();
    assert_eq!(store.snapshot().instances.len(), 1);
    store
        .reconcile_entities(vec![actor(2)], &[actor(1)])
        .unwrap();
    assert!(store.snapshot().instances.is_empty());
    assert!(Arc::ptr_eq(
        &before.materials[&material],
        &store.snapshot().materials[&material]
    ));
    assert_eq!(before.material_revision, store.snapshot().material_revision);
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

#[test]
fn distinct_polyline_endpoints_survive_expansion_before_instance_scaling() {
    for axis in 0..3 {
        for length in [1e-8, 1e-30] {
            for closed in [false, true] {
                let mut store = Store::default();
                let a = [0.; 3];
                let mut b = a;
                b[axis] = length;
                let geometry = store
                    .create_geometry(GeometrySpec::Polyline {
                        vertices: [a, a, b, b]
                            .map(|position| Vertex {
                                position,
                                color: [1.; 4],
                                uv: [0.; 2],
                                data: [0.; 4],
                            })
                            .to_vec(),
                        closed,
                    })
                    .unwrap();
                let mut instance = drawing(geometry);
                instance.transform.scale[axis] = -length.recip();
                store
                    .create(instance.clone(), DrawingPermissions::ALL)
                    .unwrap();
                store.commit();
                let expanded = super::mesh::build(&store.snapshot().geometry[&geometry]);
                let segments: Vec<_> = expanded.iter().filter(|v| v.shape[0] == 1.).collect();
                assert_eq!(segments.len(), if closed { 12 } else { 6 });
                assert_eq!(segments[0].a[..3], a);
                assert_eq!(segments[0].b[..3], b);
                let world_length =
                    (segments[0].b[axis] - segments[0].a[axis]) * instance.transform.scale[axis];
                assert!((world_length + 1.).abs() < f32::EPSILON);
            }
        }
    }
}

#[test]
fn instancing_cannot_multiply_a_full_mesh_into_unbounded_gpu_work() {
    let mut store = Store::default();
    let GeometrySpec::Triangles { vertices, .. } = mesh() else {
        unreachable!()
    };
    let indices = [0, 1, 2]
        .into_iter()
        .cycle()
        .take(mod_api::MAX_DRAW_INDICES)
        .collect();
    let geometry = store
        .create_geometry(GeometrySpec::Triangles { vertices, indices })
        .unwrap();
    let allowed = mod_api::MAX_DRAW_VERTEX_WORK / mod_api::MAX_DRAW_INDICES;
    for _ in 0..allowed {
        store
            .create(drawing(geometry), DrawingPermissions::ALL)
            .unwrap();
    }
    assert_eq!(
        store
            .create(drawing(geometry), DrawingPermissions::ALL)
            .unwrap_err()
            .detail,
        "vertex work"
    );
    store.commit();
    assert_eq!(store.snapshot().instances.len(), allowed);
}

#[test]
fn tiny_geometry_rejects_oversized_instances() {
    for length in [1e-20, 1e-30, 1e-36] {
        let mut store = Store::default();
        let geometry = store
            .create_geometry(GeometrySpec::Triangles {
                vertices: [[length, 0., 0.], [0., length, 0.], [0., 0., length]]
                    .map(|position| Vertex {
                        position,
                        color: [1.; 4],
                        uv: [0.; 2],
                        data: [0.; 4],
                    })
                    .to_vec(),
                indices: vec![0, 1, 2],
            })
            .unwrap();
        let mut valid = drawing(geometry);
        valid.transform.scale = [length.recip(); 3];
        store.create(valid, DrawingPermissions::ALL).unwrap();
        for scale in [[1e38; 3], [-1e38, 1e38, -1e38], [1e38, 0., 0.]] {
            let mut oversized = drawing(geometry);
            oversized.transform.scale = scale;
            assert_eq!(
                store
                    .create(oversized, DrawingPermissions::ALL)
                    .unwrap_err()
                    .code,
                ErrorCode::Invalid,
                "length={length}, scale={scale:?}"
            );
        }
        store.commit();
        assert_eq!(store.snapshot().instances.len(), 1);
    }
}

#[test]
fn cached_sphere_admits_extents_when_box_bounds_are_too_conservative() {
    let mut store = Store::default();
    let extent = mod_api::MAX_PRIMITIVE_EXTENT_BLOCKS;
    let geometry = store
        .create_geometry(GeometrySpec::Triangles {
            vertices: [[extent, 0., 0.], [0., extent, 0.], [0., 0., extent]]
                .map(|position| Vertex {
                    position,
                    color: [1.; 4],
                    uv: [0.; 2],
                    data: [0.; 4],
                })
                .to_vec(),
            indices: vec![0, 1, 2],
        })
        .unwrap();
    for scale in [[1.; 3], [-1., 1., -1.], [0.5; 3], [-1., 0.25, 0.], [0.; 3]] {
        let mut instance = drawing(geometry);
        instance.transform.scale = scale;
        instance.transform.translation[0] = extent * (1. - scale[0].abs());
        store.create(instance, DrawingPermissions::ALL).unwrap();
    }
    let mut outside = drawing(geometry);
    outside.transform.translation[0] = 1.;
    assert_eq!(
        store
            .create(outside, DrawingPermissions::ALL)
            .unwrap_err()
            .code,
        ErrorCode::Invalid
    );
    let mut outside = drawing(geometry);
    outside.transform.scale[0] = 1.01;
    assert_eq!(
        store
            .create(outside, DrawingPermissions::ALL)
            .unwrap_err()
            .code,
        ErrorCode::Invalid
    );
}

/// Expands an admitted polyline with uniform attributes for topology comparisons.
fn expanded_line(positions: &[[f32; 3]], closed: bool) -> Vec<super::mesh::GpuVertex> {
    let geometry = super::validation::geometry(GeometrySpec::Polyline {
        vertices: positions
            .iter()
            .map(|&position| Vertex {
                position,
                color: [1.; 4],
                uv: [0.; 2],
                data: [0.; 4],
            })
            .collect(),
        closed,
    })
    .unwrap();
    super::mesh::build(&geometry)
}

#[test]
fn repeated_polyline_points_preserve_caps_and_neighboring_joins() {
    let a = [0., 0., 0.];
    let b = [1., 0., 0.];
    let c = [1., 1., 0.];
    for points in [vec![a, a, b, c], vec![a, b, b, c], vec![a, b, c, c]] {
        assert_eq!(
            expanded_line(&points, false),
            expanded_line(&[a, b, c], false)
        );
    }
    for points in [vec![a, a, b, c], vec![a, b, b, c], vec![a, b, c, a]] {
        assert_eq!(
            expanded_line(&points, true),
            expanded_line(&[a, b, c], true)
        );
    }
}

#[test]
fn unchanged_updates_stage_nothing_and_keep_the_snapshot() {
    let mut store = Store::default();
    let geometry = store.create_geometry(mesh()).unwrap();
    let id = store
        .create(drawing(geometry), DrawingPermissions::ALL)
        .unwrap();
    store.commit();
    let before = Arc::clone(store.snapshot());
    store.begin();
    for _ in 0..32 {
        store
            .update(id, drawing(geometry), DrawingPermissions::ALL)
            .unwrap();
    }
    assert!(store.staged.is_none());
    assert_eq!(store.mutation_bytes, 0);
    store.commit();
    assert!(Arc::ptr_eq(&before, store.snapshot()));
}

#[test]
fn material_only_callbacks_publish_without_replacing_unchanged_snapshots() {
    let mut store = Store::default();
    let before = store.snapshot().clone();
    let material = store
        .create_material(crate::material::BUILTIN, DrawingPermissions::ALL)
        .unwrap();
    store.commit();
    assert!(store.snapshot().materials.contains_key(&material));
    assert!(!Arc::ptr_eq(&before, store.snapshot()));

    let before = store.snapshot().clone();
    store.begin();
    store.commit();
    assert!(Arc::ptr_eq(&before, store.snapshot()));

    store.begin();
    store.remove(material).unwrap();
    store.rollback();
    assert!(Arc::ptr_eq(&before, store.snapshot()));

    store.begin();
    store.remove(material).unwrap();
    store.commit();
    assert!(store.snapshot().materials.is_empty());
    assert!(!Arc::ptr_eq(&before, store.snapshot()));
    assert!(before.materials.contains_key(&material));
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
    let mut foreign = Store::default();
    let other = foreign
        .create_material(source, DrawingPermissions::ALL)
        .unwrap();
    let mut instance = drawing(geometry);
    instance.material = Some(other);
    assert_eq!(
        store
            .create(instance.clone(), DrawingPermissions::ALL)
            .unwrap_err()
            .code,
        ErrorCode::StaleHandle
    );
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

#[test]
fn rolling_back_material_removal_does_not_double_charge_a_live_resource() {
    let mut store = Store::default();
    let material = store
        .create_material(crate::material::BUILTIN, DrawingPermissions::ALL)
        .unwrap();
    store.commit();
    for _ in 0..3 {
        store.begin();
        store.remove(material).unwrap();
        store.rollback();
    }
    for _ in 1..mod_api::MAX_DRAW_MATERIALS {
        store.begin();
        store
            .create_material(crate::material::BUILTIN, DrawingPermissions::ALL)
            .unwrap();
        store.commit();
    }
    assert_eq!(
        store.snapshot().materials.len(),
        mod_api::MAX_DRAW_MATERIALS
    );
}

#[test]
fn entity_observations_commit_without_geometry_or_instance_edits() {
    let mut store = Store::default();
    store.bind_session(1, 0);
    store.reconcile_entities(vec![actor(1)], &[]).unwrap();
    let observed = store.observe_entities().unwrap();
    assert!(store.snapshot().entities.is_empty());
    store.commit();
    assert_eq!(
        store.snapshot().entities.get(&observed[0].0),
        Some(&actor(1))
    );

    let previous = Arc::clone(store.snapshot());
    store.begin();
    assert_eq!(store.observe_entities().unwrap(), observed);
    store.commit();
    assert!(Arc::ptr_eq(&previous, store.snapshot()));

    let geometry = store.create_geometry(mesh()).unwrap();
    let mut instance = drawing(geometry);
    instance.anchor = Anchor::Entity(EntityAnchor {
        entity: observed[0].0,
        body_rotation: false,
    });
    store.create(instance, DrawingPermissions::ALL).unwrap();
}

#[test]
fn entity_reference_release_commits_without_geometry_or_instance_edits() {
    let mut store = Store::default();
    store.reconcile_entities(vec![actor(1)], &[]).unwrap();
    let observed = store.observe_entities().unwrap();
    store.create_geometry(mesh()).unwrap();
    store.commit();
    let previous = Arc::clone(store.snapshot());
    store.begin();
    store.remove(observed[0].0).unwrap();
    store.rollback();
    assert!(Arc::ptr_eq(&previous, store.snapshot()));
    store.begin();
    store.remove(observed[0].0).unwrap();
    store.commit();
    assert!(store.snapshot().entities.is_empty());
    assert_eq!(
        store.remove(observed[0].0).unwrap_err().code,
        ErrorCode::StaleHandle
    );
    store.begin();
    assert_ne!(store.observe_entities().unwrap()[0].0, observed[0].0);
}
