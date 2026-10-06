use super::*;
use render_api::primitive_shapes::*;

#[path = "alloc_count.rs"]
mod alloc_count;
#[path = "bench.rs"]
mod bench;

/// Builds a small patch without hiding omitted-field semantics behind fixture defaults.
fn update(id: u64, kind: PrimitiveShapeKind) -> PrimitiveShapeUpdate {
    PrimitiveShapeUpdate {
        network_id: id,
        kind,
        location: None,
        rotation: None,
        scale: None,
        color: None,
        total_time_left: None,
        maximum_render_distance: None,
        dimension: None,
        attached_actor: None,
        data: PrimitiveShapeData::None,
    }
}

/// Sends one normalized patch through the same store entry point used by the client.
fn apply(store: &mut PrimitiveShapeStore, patch: PrimitiveShapeUpdate) {
    store.apply(PrimitiveShapesEvent {
        changes: vec![PrimitiveShapeChange::Upsert(patch)],
        skipped_entries: 0,
    });
}

/// Consumes all attributable GPU changes, as a render extraction would.
fn flush(store: &mut PrimitiveShapeStore) -> PrimitiveUploadStats {
    let stats = store.drain_uploads(|_, _, _, _| {});
    store.actors.drain(|_, _| {});
    store.text_records.drain(|_, _| {});
    stats
}

#[test]
fn steady_state_does_no_allocations_uploads_or_instance_rebuilds() {
    let mut store = PrimitiveShapeStore::default();
    for id in 0..10_000 {
        apply(&mut store, update(id, PrimitiveShapeKind::Sphere));
    }
    flush(&mut store);
    let rebuilds = store.instance_rebuilds;
    let allocations = alloc_count::thread_allocations();
    for _ in 0..1_000 {
        assert!(!store.has_changes());
        assert_eq!(flush(&mut store), PrimitiveUploadStats::default());
        assert!(store.take_text_changes().is_empty());
        store.update_actors(|_| panic!("no actors are attached"));
    }
    assert_eq!(alloc_count::thread_allocations(), allocations);
    assert_eq!(store.instance_rebuilds, rebuilds);
}

#[test]
fn sparse_churn_uploads_each_changed_slot_once_and_keeps_indices() {
    let mut store = PrimitiveShapeStore::default();
    for id in 0..1_000 {
        apply(&mut store, update(id, PrimitiveShapeKind::Line));
    }
    flush(&mut store);
    for id in (0..1_000).step_by(10) {
        let mut patch = update(id, PrimitiveShapeKind::Line);
        patch.location = Some([id as f32, 5.0, 3.0]);
        apply(&mut store, patch.clone());
        apply(&mut store, patch);
    }
    let mut touched = Vec::new();
    let stats = store.drain_uploads(|batch, _, start, values| {
        assert_eq!(batch, 0);
        touched.extend(start..start + values.len() as u32);
    });
    assert_eq!(touched, (0..1_000).step_by(10).collect::<Vec<_>>());
    assert_eq!(stats.slots, 100);
    assert_eq!(stats.bytes, 100 * std::mem::size_of::<PrimitiveInstance>());
    assert_eq!(store.batches()[0].instances.values.len(), 1_000);
}

#[test]
fn patch_preserves_omitted_fields_and_original_concrete_kind() {
    let mut store = PrimitiveShapeStore::default();
    let mut patch = update(7, PrimitiveShapeKind::Box);
    patch.location = Some([3.0, 4.0, 5.0]);
    patch.scale = Some(2.0);
    patch.color = Some([1.0, 0.0, 0.0, 128.0 / 255.0]);
    patch.dimension = Some(-123);
    patch.data = PrimitiveShapeData::Box {
        bounds: [2.0, 3.0, 4.0],
    };
    apply(&mut store, patch);
    let mut patch = update(7, PrimitiveShapeKind::Sphere);
    patch.rotation = Some([90.0; 3]);
    patch.data = PrimitiveShapeData::Segments(32);
    apply(&mut store, patch);
    let state = store.get(7).unwrap();
    assert_eq!(state.kind, PrimitiveShapeKind::Box);
    assert_eq!(state.location, [3.0, 4.0, 5.0]);
    assert_eq!(state.dimension, -123);
    let instance = state.instance(u32::MAX);
    assert_eq!(instance.transform[0], [4.0, 0.0, 0.0, 0.0]);
    assert_eq!(instance.transform[1], [0.0, 6.0, 0.0, 0.0]);
    assert_eq!(instance.transform[2], [0.0, 0.0, 8.0, 0.0]);
    assert_eq!(instance.color, [1.0, 0.0, 0.0, 128.0 / 255.0]);
}

#[test]
fn removal_is_idempotent_and_reuses_only_the_removed_slot() {
    let mut store = PrimitiveShapeStore::default();
    for id in 0..3 {
        apply(&mut store, update(id, PrimitiveShapeKind::Box));
    }
    flush(&mut store);
    for id in [1, 1, 999] {
        store.apply(PrimitiveShapesEvent {
            changes: vec![PrimitiveShapeChange::Remove { network_id: id }],
            skipped_entries: 0,
        });
    }
    assert_eq!(flush(&mut store).slots, 1);
    assert_eq!(store.batches()[0].instances.values[1].meta[0], 0);
    apply(&mut store, update(20, PrimitiveShapeKind::Box));
    assert_eq!(store.batches()[0].instances.values.len(), 3);
    assert_eq!(store.batches()[0].instances.values[1].meta[0], 1);
    assert_eq!(flush(&mut store).slots, 1);
}

#[test]
fn removing_all_shapes_retires_draws_but_keeps_capacity_for_reuse() {
    let mut store = PrimitiveShapeStore::default();
    apply(&mut store, update(1, PrimitiveShapeKind::Line));
    assert!(!store.batches()[0].instances.is_empty());
    store.apply(PrimitiveShapesEvent {
        changes: vec![PrimitiveShapeChange::Remove { network_id: 1 }],
        skipped_entries: 0,
    });
    assert!(store.batches()[0].instances.is_empty());
    assert_eq!(store.batches()[0].instances.values.len(), 1);
    apply(&mut store, update(2, PrimitiveShapeKind::Line));
    assert!(!store.batches()[0].instances.is_empty());
    assert_eq!(store.batches()[0].instances.values.len(), 1);
}

#[test]
fn attachments_share_position_slots_without_rebuilding_shapes() {
    let mut store = PrimitiveShapeStore::default();
    for id in 0..100 {
        let mut patch = update(id, PrimitiveShapeKind::Sphere);
        patch.attached_actor = Some(42);
        apply(&mut store, patch);
    }
    flush(&mut store);
    assert_eq!(store.actors.values.len(), 1);
    let rebuilds = store.instance_rebuilds;
    let mut reads = 0;
    store.update_actors(|id| {
        assert_eq!(id, 42);
        reads += 1;
        Some([1.0, 2.0, 3.0])
    });
    assert_eq!(reads, 1);
    assert_eq!(store.instance_rebuilds, rebuilds);
    assert_eq!(
        store
            .drain_uploads(|_, _, _, _| panic!("actor motion must not touch shape slots"))
            .bytes,
        0
    );
    let mut bytes = 0;
    store
        .actors
        .drain(|_, records| bytes += std::mem::size_of_val(records));
    assert_eq!(bytes, std::mem::size_of::<PrimitiveActor>());
    store.update_actors(|_| Some([1.0, 2.0, 3.0]));
    assert!(!store.has_changes());
}

#[test]
fn segment_changes_move_only_the_changed_shape_to_a_shared_variant() {
    let mut store = PrimitiveShapeStore::default();
    for id in 0..100 {
        apply(&mut store, update(id, PrimitiveShapeKind::Circle));
    }
    flush(&mut store);
    let mut patch = update(50, PrimitiveShapeKind::Circle);
    patch.data = PrimitiveShapeData::Segments(32);
    apply(&mut store, patch);
    assert_eq!(flush(&mut store).slots, 2);
    assert_eq!(store.batches().len(), 2);
    assert_eq!(store.batches()[0].instances.values[50].meta[0], 0);
    assert_eq!(store.batches()[1].key.segments, 32);
}

#[test]
fn text_movement_retains_atlas_quads_and_changes_only_its_shape_slot() {
    let mut store = PrimitiveShapeStore::default();
    let mut patch = update(8, PrimitiveShapeKind::Text);
    patch.data = PrimitiveShapeData::Text(PrimitiveText {
        text: "test".into(),
        use_rotation: false,
        background_color: None,
        line_gap_height: 0.0,
        depth_test: true,
        show_backface: true,
        show_text_backface: true,
    });
    apply(&mut store, patch);
    assert_eq!(store.take_text_changes().len(), 1);
    store.set_text_records(
        8,
        vec![PrimitiveTextRecord {
            rect: [0.0, 0.0, 8.0, 8.0],
            ..Default::default()
        }],
    );
    flush(&mut store);
    let mut patch = update(8, PrimitiveShapeKind::Text);
    patch.location = Some([8.0, 9.0, 10.0]);
    apply(&mut store, patch);
    assert!(store.take_text_changes().is_empty());
    assert!(store.text_records.is_clean());
    assert_eq!(flush(&mut store).slots, 1);
}

#[test]
fn explicit_text_rotation_applies_x_then_y_then_z() {
    let mut state = PrimitiveState::new(PrimitiveShapeKind::Text);
    state.rotation = [90.0, 90.0, 0.0];
    let instance = state.instance(u32::MAX);
    let point = glam::Mat4::from_cols_array_2d(&instance.transform).transform_point3(glam::Vec3::Y);
    assert!((point - glam::Vec3::X).length() < 0.00001);
}

#[test]
fn time_left_is_metadata_until_the_server_removes_the_id() {
    let mut store = PrimitiveShapeStore::default();
    let mut patch = update(19, PrimitiveShapeKind::Box);
    patch.total_time_left = Some(-5.0);
    apply(&mut store, patch);
    assert_eq!(store.get(19).unwrap().total_time_left, Some(-5.0));
    assert_eq!(store.batches()[0].instances.values[0].lifetime[0], -1.0);
    apply(&mut store, update(20, PrimitiveShapeKind::Line));
    assert!(store.get(19).is_some());
    let mut patch = update(19, PrimitiveShapeKind::Box);
    patch.total_time_left = Some(0.0);
    apply(&mut store, patch);
    assert_eq!(store.get(19).unwrap().total_time_left, None);
    store.apply(PrimitiveShapesEvent {
        changes: vec![PrimitiveShapeChange::Remove { network_id: 19 }],
        skipped_entries: 0,
    });
    assert!(store.get(19).is_none());
}

#[test]
fn dynamic_text_common_changes_queue_once_and_literal_changes_do_not() {
    let mut store = PrimitiveShapeStore::default();
    let mut patch = update(8, PrimitiveShapeKind::Text);
    patch.data = PrimitiveShapeData::Text(PrimitiveText {
        text: "text".into(),
        use_rotation: false,
        background_color: None,
        line_gap_height: 0.0,
        depth_test: false,
        show_backface: true,
        show_text_backface: false,
    });
    apply(&mut store, patch);
    store.take_text_changes();
    store.set_text_dynamic(8, true);
    for position in [1.0, 2.0] {
        let mut patch = update(8, PrimitiveShapeKind::Text);
        patch.location = Some([position, 0.0, 0.0]);
        apply(&mut store, patch);
    }
    assert_eq!(store.take_text_changes().len(), 1);
    assert!(store.take_text_changes().is_empty());
    store.set_text_dynamic(8, false);
    let mut patch = update(8, PrimitiveShapeKind::Text);
    patch.location = Some([3.0, 0.0, 0.0]);
    apply(&mut store, patch);
    assert!(store.take_text_changes().is_empty());
}
