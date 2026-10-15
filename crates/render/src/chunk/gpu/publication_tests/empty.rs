//! Empty publication receipts stay scoped to their source and retire exactly once.

use super::*;

#[test]
fn a_late_empty_gpu_completion_cannot_enter_the_next_sources_receipts() {
    use bevy::ecs::system::RunSystemOnce;
    use render_scene::terrain_publication::TerrainPublications;
    let key = SubChunkKey::new(0, 7, 0, 9);
    let old = TerrainPublications::default();
    let next = TerrainPublications::default();
    old.retain(std::iter::once(key));
    next.retain(std::iter::once(key));
    let config = render_api::PublicationServiceConfig::DEFAULT;
    let allowance = render_api::PublicationAllowance::new(config);
    allowance.begin_frame(1, 1, 0, 1, config.maximum_frame_items);
    let gpu_removals = ChunkGpuRemovalQueue::default();
    gpu_removals
        .push(PendingGpuRemoval {
            publications: Some(old.clone()),
            kind: render_api::TerrainRemovalKind::Empty,
            key,
            token: Some(ChunkUploadToken {
                generation: 41,
                dirty_since: Instant::now(),
            }),
            priority: ChunkUploadPriority::new(0.0),
            permit: allowance
                .try_admit_zero_byte()
                .unwrap()
                .into_handoff()
                .unwrap()
                .into_render_entity()
                .unwrap(),
        })
        .unwrap_or_else(|_| panic!("the empty mailbox has room"));
    let mut app = noop_gpu_publication_app(ChunkUploadAcknowledgements::default(), gpu_removals);
    app.init_resource::<TerrainCoverage>()
        .insert_resource(render_scene::FarTerrainScene {
            publications: Some(next.clone()),
            ..Default::default()
        });
    app.world_mut()
        .run_system_once(crate::chunk::coverage::begin_terrain_coverage)
        .unwrap();
    app.world_mut().run_system_once(prepare_gpu_chunks).unwrap();
    assert_eq!(old.changed_empty(None).unwrap().1[&key], 41);
    assert!(next.changed_empty(None).unwrap().1.is_empty());
    assert_eq!(allowance.live_permits(), 0);
}

#[test]
fn gpu_preparation_acknowledges_and_retires_a_permitted_known_air_removal_exactly_once() {
    use bevy::ecs::system::RunSystemOnce;

    let key = SubChunkKey::new(0, 7, 0, 9);
    let token = ChunkUploadToken {
        generation: 41,
        dirty_since: Instant::now(),
    };
    let allowance =
        render_api::PublicationAllowance::new(render_api::PublicationServiceConfig::DEFAULT);
    allowance.begin_frame(
        1,
        1,
        0,
        1,
        render_api::PublicationServiceConfig::DEFAULT.maximum_frame_items,
    );
    let permit = allowance
        .try_admit_zero_byte()
        .unwrap()
        .into_handoff()
        .unwrap()
        .into_render_entity()
        .unwrap();
    let acknowledgements = ChunkUploadAcknowledgements::default();
    let gpu_removals = ChunkGpuRemovalQueue::default();
    assert!(
        gpu_removals
            .push(PendingGpuRemoval {
                publications: None,
                kind: render_api::TerrainRemovalKind::Empty,
                key,
                token: Some(token),
                priority: ChunkUploadPriority::new(0.0),
                permit,
            })
            .is_ok()
    );
    let mut app = noop_gpu_publication_app(acknowledgements.clone(), gpu_removals.clone());

    app.world_mut().run_system_once(prepare_gpu_chunks).unwrap();

    let completed = acknowledgements.drain();
    assert_eq!(completed.len(), 1);
    assert_eq!(completed[0].key, key);
    assert_eq!(completed[0].token, token);
    assert!(acknowledgements.is_empty());
    assert_eq!(gpu_removals.pending_len(), 0);
    assert_eq!(allowance.live_permits(), 0);

    app.world_mut().run_system_once(prepare_gpu_chunks).unwrap();
    assert!(acknowledgements.drain().is_empty());
}
