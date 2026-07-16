use super::*;

fn assert_indirect_view_query_is_static<C>()
where
    C: RenderCommand<Opaque3d, ViewQuery = OpaqueChunkViewQuery>,
{
}

#[test]
fn indirect_batch_staging_cannot_invalidate_the_view_query() {
    assert_indirect_view_query_is_static::<DrawPackedChunksIndirect>();
}

fn active_visibility_probe_for_view(
    selected_view: Entity,
    draw_mode: OpaqueDrawMode,
    keys: [SubChunkKey; 2],
) -> ActiveVisibilityFrameProbe {
    let mut input = VisibilityDiagnosticsInput::new(true);
    input.advance(keys, keys);
    let active = ActiveVisibilityFrameProbe::default();
    active.begin(VisibilityFrameProbe::begin_for_view(
        input,
        selected_view,
        ExtractedCameraIdentity {
            stable_id: 1,
            pose_hash: 2,
            frustum_hash: 3,
        },
        crate::ExtractedViewGenerations::new(1, 1),
        draw_mode,
        keys,
        8,
    ));
    active
}

#[test]
fn direct_render_command_seam_rejects_secondary_camera_submission() {
    let selected_view = Entity::from_bits(1);
    let secondary_view = Entity::from_bits(2);
    let selected_key = SubChunkKey::new(0, 1, 2, 3);
    let secondary_key = SubChunkKey::new(0, 4, 5, 6);
    let probe = active_visibility_probe_for_view(
        selected_view,
        OpaqueDrawMode::Direct,
        [selected_key, secondary_key],
    );

    assert!(!record_visibility_direct_submission(
        &probe,
        secondary_view,
        secondary_key,
    ));
    assert!(record_visibility_direct_submission(
        &probe,
        selected_view,
        selected_key,
    ));

    let snapshot = probe.take_completed().unwrap();
    assert_eq!(
        snapshot.submitted_opaque,
        Some(VisibilityKeyDigest::from_keys([selected_key]))
    );
}

#[test]
fn mdi_render_command_seam_rejects_secondary_camera_submission() {
    let selected_view = Entity::from_bits(1);
    let secondary_view = Entity::from_bits(2);
    let selected_key = SubChunkKey::new(0, 1, 2, 3);
    let secondary_key = SubChunkKey::new(0, 4, 5, 6);
    let probe = active_visibility_probe_for_view(
        selected_view,
        OpaqueDrawMode::MultiDrawIndirect,
        [selected_key, secondary_key],
    );

    assert_eq!(
        record_visibility_mdi_submissions(&probe, secondary_view, [secondary_key]),
        0
    );
    assert_eq!(
        record_visibility_mdi_submissions(&probe, selected_view, [selected_key]),
        1
    );

    let snapshot = probe.take_completed().unwrap();
    assert_eq!(
        snapshot.submitted_opaque,
        Some(VisibilityKeyDigest::from_keys([selected_key]))
    );
}

#[test]
fn missing_or_empty_indirect_batch_has_no_draw_arguments() {
    let mut world = World::new();
    let view = world.spawn_empty().id();
    let mut batches = ChunkIndirectBatches::default();

    assert_eq!(indirect_batch_draw_args(&batches, view), None);

    batches.0.insert(
        view,
        ChunkIndirectBatch {
            visible_entities: Vec::new(),
            drawn_allocations: Vec::new(),
            indirect_offset: 40,
            command_count: 0,
        },
    );
    assert_eq!(indirect_batch_draw_args(&batches, view), None);

    batches.0.get_mut(&view).unwrap().command_count = 3;
    assert_eq!(indirect_batch_draw_args(&batches, view), Some((40, 3)));
}
