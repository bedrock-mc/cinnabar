//! Geometry failures must reject admission without losing the next publication's uploads.

use super::{resources::prepare_ui_resources, *};
use bevy::ecs::system::RunSystemOnce;
use render_model::{
    UI_DYNAMIC_PAGE_SIDE, UI_PLAYER_SKIN_PAGE_OFFSET, UiScissor, UiTextureCatalog, UiTexturePage,
};
use std::{cell::Cell, rc::Rc};

/// Builds a triangle on the resizable player-skin slot in a valid dynamic catalog.
fn input() -> UiRenderInput {
    let blank = UiTexturePage::owned(
        [UI_DYNAMIC_PAGE_SIDE; 2],
        vec![255; (UI_DYNAMIC_PAGE_SIDE * UI_DYNAMIC_PAGE_SIDE * 4) as usize].into(),
    )
    .unwrap();
    UiRenderInput {
        revision: 1,
        viewport_size: [64; 2],
        safe_area: [0; 4],
        vertices: vec![
            UiRenderVertex {
                position: [1.0; 2],
                clip_z: 0.0,
                clip_w: 1.0,
                uv: [0.0; 2],
                color: [255; 4],
                style_flags: 0,
                alpha_cutoff: -1.0,
                model_light: 1.0,
                overlay_color: [0.0; 4],
            };
            3
        ]
        .into(),
        indices: Arc::from([0, 1, 2]),
        batches: Arc::from([UiRenderBatch::new(
            UI_PLAYER_SKIN_PAGE_OFFSET as u32,
            UiScissor::new(0, 0, 64, 64),
            0,
            3,
            0,
        )]),
        textures: Arc::new(
            UiTextureCatalog::new(vec![blank; UI_PLAYER_SKIN_PAGE_OFFSET + 1], 0).unwrap(),
        ),
    }
}

/// Publishes through the scene owner before running the actual GPU preparation system.
fn publish(world: &mut World, input: UiRenderInput) {
    let stats = world.resource::<UiRenderStatsResource>().clone();
    world
        .resource_mut::<UiRenderSceneResource>()
        .publish(input, &stats)
        .unwrap();
}

#[test]
fn geometry_oom_after_texture_resize_rejects_and_recovers_publication() {
    for failed_stage in [
        "shared bounded UI vertex arena",
        "shared bounded UI index arena",
        "UI vertex upload",
        "UI index upload",
    ] {
        for revert in [false, true] {
            let mut world = ordered_command_tests::binding_world();
            let original = input();
            publish(&mut world, original.clone());
            world.run_system_once(prepare_ui_resources).unwrap();

            let mut resized = original.clone();
            resized.revision = 2;
            resized.vertices = vec![original.vertices[0]; 6].into();
            resized.indices = Arc::from([0, 1, 2, 3, 4, 5]);
            resized.batches = Arc::from([UiRenderBatch {
                index_count: 6,
                ..original.batches[0]
            }]);
            let mut pages = original.textures.pages().to_vec();
            let side = render_api::CLASSIC_SKIN_SIDE as u32;
            pages[UI_PLAYER_SKIN_PAGE_OFFSET] =
                UiTexturePage::owned([side; 2], vec![255; (side * side * 4) as usize].into())
                    .unwrap();
            resized.textures = Arc::new(original.textures.replace_dynamic(pages).unwrap());
            publish(&mut world, resized);
            let reached = Rc::new(Cell::new(false));
            let observed = reached.clone();
            let failure = crate::gpu_allocation::inject(move |label, _| {
                if label == failed_stage {
                    observed.set(true);
                    true
                } else {
                    label == "bounded UI dimension bucket"
                }
            });
            world.run_system_once(prepare_ui_resources).unwrap();
            let gpu = world.resource::<UiGpu>();
            assert_eq!(gpu.accepted_revision, None, "{failed_stage}");
            assert!(gpu.batches.is_empty());
            assert!(gpu.animation_damage.is_empty());
            assert_eq!(gpu.last_admitted_revision, Some(1));
            assert!(
                reached.get(),
                "failure boundary not reached: {failed_stage}"
            );
            let stats = world.resource::<UiRenderStatsResource>().snapshot();
            assert_eq!(stats.rejected_revision, Some(2));
            assert!(stats.rejected_reason.is_some());
            assert_eq!(stats.draw_calls, 0);
            drop(failure);

            if revert {
                let mut restored = original;
                restored.revision = 3;
                publish(&mut world, restored);
            }
            // Geometry can recover immediately even while the texture plan is in backoff.
            world.run_system_once(prepare_ui_resources).unwrap();
            let stats = world.resource::<UiRenderStatsResource>().snapshot();
            assert_eq!(stats.accepted_revision, Some(if revert { 3 } else { 2 }));
            assert_eq!(stats.rejected_revision, None);
            assert_eq!(stats.uploaded_vertices, if revert { 3 } else { 6 });
            assert_eq!(stats.uploaded_indices, if revert { 3 } else { 6 });
            let writes = world.resource::<UiGpu>().geometry_writes;
            world.run_system_once(prepare_ui_resources).unwrap();
            assert_eq!(world.resource::<UiGpu>().geometry_writes, writes);
        }
    }
}

#[test]
fn geometry_upload_oom_retries_changed_bytes_in_retained_arenas() {
    for failed_stage in ["UI vertex upload", "UI index upload"] {
        let mut world = ordered_command_tests::binding_world();
        let original = input();
        publish(&mut world, original.clone());
        world.run_system_once(prepare_ui_resources).unwrap();
        let mut changed = original;
        changed.revision = 2;
        Arc::make_mut(&mut changed.vertices)[1].position[0] = 2.0;
        changed.indices = Arc::from([2, 1, 0]);
        publish(&mut world, changed);

        let failure = crate::gpu_allocation::inject(move |label, _| label == failed_stage);
        world.run_system_once(prepare_ui_resources).unwrap();
        assert_eq!(
            world.resource::<UiGpu>().accepted_revision,
            None,
            "{failed_stage}"
        );
        drop(failure);
        world.run_system_once(prepare_ui_resources).unwrap();
        let stats = world.resource::<UiRenderStatsResource>().snapshot();
        assert_eq!(stats.accepted_revision, Some(2));
        assert_eq!(stats.uploaded_vertices, 3);
        assert_eq!(stats.uploaded_indices, 3);
    }
}
