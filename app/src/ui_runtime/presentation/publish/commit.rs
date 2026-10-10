//! Pure UI rasterization, layout and publication after input enqueue.
use super::*;

/// Publishes the UI captured during preparation.
#[allow(clippy::too_many_arguments)]
pub(crate) fn publish_ui_runtime(
    player_runtime: Res<crate::player_runtime::PlayerRuntime>,
    mut runtime: ResMut<UiRuntime>,
    mut prepared: ResMut<PreparedUiPublication>,
    mut presentation: ResMut<UiPresentationRuntime>,
    mut scene: ResMut<render::UiRenderSceneResource>,
    stats: Res<render::UiRenderStatsResource>,
    hand_rig: Res<render::HandRigScene>,
    nametag_scene: Option<ResMut<render::NametagSceneResource>>,
    mut hand: crate::presentation::viewmodel::ViewmodelPublish,
    profiler: Option<Res<render::RuntimeStageProfiler>>,
) {
    let Some(prepared) = prepared.0.take() else {
        return;
    };
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(render::RuntimeStage::UiPublication));
    let input = match client_ui::ui_runtime::presentation::render_prepared_ui(
        &player_runtime,
        &mut runtime,
        &mut presentation,
        prepared,
    ) {
        Ok(input) => input,
        Err(error) => {
            hand.clear();
            presentation.record_frame_failure(&error);
            return;
        }
    };
    if let Some(mut scene) = nametag_scene {
        scene.0 = presentation.nametag_scene();
    }
    if !hand_rig.is_active() {
        hand.bind_cpu_fallback(
            &input,
            presentation.cpu_empty_hand_fallback(),
            presentation.hud_frame().held_item_icon,
        );
    }
    if let Err(error) = scene.publish(input, &stats) {
        hand.clear();
        presentation.record_frame_failure(&UiPresentationError::Render(error));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;
    use bevy::prelude::World;

    /// Supplies only CPU presentation owners, without a window or network connection.
    fn world() -> World {
        let mut world = World::new();
        world.insert_resource(crate::player_runtime::PlayerRuntime::new(1));
        world.insert_resource(UiRuntime::new(1));
        let mut presentation =
            UiPresentationRuntime::new(crate::ui_runtime::presentation::tests::fixture_font())
                .unwrap();
        presentation.set_menu_view(Some(client_ui::menu::MenuView::new(true, "Fixture".into())));
        world.insert_resource(presentation);
        world.init_resource::<PreparedUiPublication>();
        world.init_resource::<render::UiRenderSceneResource>();
        world.init_resource::<render::UiRenderStatsResource>();
        world.init_resource::<ClientWorld>();
        world.init_resource::<render::HandRigScene>();
        world
    }

    /// Publishes a captured frame through the same system that drives a connected session.
    fn publish(world: &mut World, size: [u32; 2]) {
        let inventory = world
            .resource::<UiRuntime>()
            .capture_presentation_inventory(
                world.resource::<crate::player_runtime::PlayerRuntime>(),
            );
        world.resource_mut::<PreparedUiPublication>().0 = Some(PendingUiPublication {
            inventory,
            preview: PreviewCapture {
                skin: None,
                ready: false,
                pose: Default::default(),
                shown: false,
                hands: false,
            },
            item_icons: (None, None),
            now_millis: 0,
            physical_size: size,
            dpi_scale: DpiScale::new(1.0).unwrap(),
        });
        world.run_system_once(publish_ui_runtime).unwrap();
    }

    #[test]
    fn rejected_render_publication_does_not_end_the_session() {
        let mut world = world();
        world
            .resource_mut::<render::UiRenderSceneResource>()
            .revision = u64::MAX;
        publish(&mut world, [1280, 720]);
        assert!(
            world.resource::<ClientWorld>().fatal_error.is_none(),
            "{:?}",
            world.resource::<ClientWorld>().fatal_error
        );
        assert!(
            world
                .resource::<render::UiRenderSceneResource>()
                .input
                .is_none()
        );
    }

    #[test]
    fn overflowing_text_publishes_without_ending_the_session() {
        let mut world = world();
        let mut view = client_ui::menu::MenuView::new(true, "Fixture".into());
        view.screen = client_ui::menu::MenuScreen::Settings;
        world
            .resource_mut::<UiPresentationRuntime>()
            .set_menu_view(Some(view));
        publish(&mut world, [254, 124]);
        assert!(
            world.resource::<ClientWorld>().fatal_error.is_none(),
            "{:?}",
            world.resource::<ClientWorld>().fatal_error
        );
        assert!(
            world
                .resource::<render::UiRenderSceneResource>()
                .input
                .is_some()
        );
        publish(&mut world, [1280, 720]);
        assert!(
            world.resource::<ClientWorld>().fatal_error.is_none(),
            "{:?}",
            world.resource::<ClientWorld>().fatal_error
        );
    }

    #[test]
    fn rejected_ui_frame_does_not_end_the_session_and_next_frame_publishes() {
        for size in [[0, 0], [1, 1]] {
            let mut world = world();
            publish(&mut world, size);
            assert!(
                world.resource::<ClientWorld>().fatal_error.is_none(),
                "{:?}",
                world.resource::<ClientWorld>().fatal_error
            );
            assert_eq!(
                world
                    .resource::<UiPresentationRuntime>()
                    .rejected_frame_count(),
                1
            );
            publish(&mut world, [1280, 720]);
            assert!(
                world.resource::<ClientWorld>().fatal_error.is_none(),
                "{:?}",
                world.resource::<ClientWorld>().fatal_error
            );
            assert!(
                world
                    .resource::<render::UiRenderSceneResource>()
                    .input
                    .is_some()
            );
        }
    }
}
