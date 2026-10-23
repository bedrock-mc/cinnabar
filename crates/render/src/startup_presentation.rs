//! Confirms startup only after primary-window presentation and GPU completion.

#[path = "startup_presentation/timing.rs"]
mod timing;

use bevy::{
    prelude::{
        App, ContainsEntity, Entity, IntoScheduleConfigs, Plugin, Query, Res, ResMut, Resource,
        With,
    },
    render::{
        Extract, ExtractSchedule, Render, RenderApp, RenderSystems,
        renderer::{RenderQueue, render_system},
        view::window::ExtractedWindow,
    },
    window::PrimaryWindow,
};
use render_scene::startup_presentation::StartupPresentation;

/// Installs a bounded render-to-main startup completion signal for native windows.
pub struct StartupPresentationPlugin;

impl Plugin for StartupPresentationPlugin {
    /// Shares the feedback resource and orders surface observation around render submission.
    fn build(&self, app: &mut App) {
        timing::install(app);
        app.init_resource::<StartupPresentation>();
        let feedback = app.world().resource::<StartupPresentation>().clone();
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        crate::device_poll::install(render_app);
        render_app
            .insert_resource(feedback)
            .init_resource::<PresentationProbe>()
            .init_resource::<ExtractedSurfaceEpoch>()
            .add_systems(ExtractSchedule, extract_surface_epoch)
            .add_systems(
                Render,
                capture_surface
                    .in_set(RenderSystems::Render)
                    .before(render_system),
            )
            .add_systems(
                Render,
                observe_presentation.in_set(crate::device_poll::FrameSubmissions),
            );
    }
}

/// The surface generation belonging to this extracted frame, even if main has advanced.
#[derive(Default, Resource)]
struct ExtractedSurfaceEpoch(Option<u64>);

/// Snapshots generation while main-world window extraction is still exclusive.
fn extract_surface_epoch(
    feedback: Extract<Res<StartupPresentation>>,
    mut epoch: ResMut<ExtractedSurfaceEpoch>,
) {
    epoch.0 = feedback.content_epoch();
}

#[derive(Default, Resource)]
struct PresentationProbe {
    surface: Option<Entity>,
    submitted: u8,
    content_submitted: Option<u64>,
    offscreen_submitted: bool,
}

impl PresentationProbe {
    /// Admits a completion callback only for a captured surface consumed by presentation.
    fn take_presented(&mut self, consumed: bool) -> bool {
        if self.surface.take().is_none()
            || !consumed
            || self.submitted >= StartupPresentation::REQUIRED_FRAMES
        {
            return false;
        }
        self.submitted += 1;
        true
    }
}

/// Remembers an acquired primary-window surface immediately before the render graph runs.
fn capture_surface(
    windows: Query<(Entity, &ExtractedWindow), With<PrimaryWindow>>,
    mut probe: ResMut<PresentationProbe>,
    epoch: Res<ExtractedSurfaceEpoch>,
) {
    probe.surface = if probe.submitted < StartupPresentation::REQUIRED_FRAMES
        || probe.content_submitted != epoch.0
    {
        windows
            .iter()
            .find_map(|(entity, window)| window.swap_chain_texture.is_some().then_some(entity))
    } else {
        None
    };
}

/// Present takes the surface texture; GPU feedback arrives only after that frame's work finishes.
fn observe_presentation(
    windows: Query<(&bevy::render::sync_world::MainEntity, &ExtractedWindow), With<PrimaryWindow>>,
    mut probe: ResMut<PresentationProbe>,
    layers: Query<(
        &bevy::render::camera::ExtractedCamera,
        &crate::ui_render::composite::UiLayerTexture,
    )>,
    queue: Res<RenderQueue>,
    feedback: Res<StartupPresentation>,
    epoch: Res<ExtractedSurfaceEpoch>,
) {
    if !probe.offscreen_submitted
        && let Some(timing) =
            render_scene::startup_timing::current().filter(|timing| timing.is_offscreen())
        && layers.iter().any(|(camera, layer)| {
            matches!(
                &camera.target,
                Some(bevy::camera::NormalizedRenderTarget::Image(_))
            ) && layer.presented_complete_content()
        })
    {
        probe.offscreen_submitted = true;
        let timing = timing.clone();
        crate::device_poll::on_frame_complete(&queue, move || {
            timing.record_offscreen_content(web_time::Instant::now())
        });
    }
    let presented_window = probe
        .surface
        .and_then(|entity| windows.get(entity).ok())
        .filter(|(_, window)| window.swap_chain_texture.is_none())
        .map(|(main, _)| main.id());
    let consumed = presented_window.is_some();
    if let Some(epoch) = epoch.0
        && consumed
        && probe.content_submitted != Some(epoch)
        && layers.iter().any(|(camera, layer)| {
            matches!(&camera.target, Some(bevy::camera::NormalizedRenderTarget::Window(window)) if Some(window.entity()) == presented_window)
                && layer.presented_complete_content()
        })
    {
        probe.content_submitted = Some(epoch);
        let feedback = feedback.clone();
        crate::device_poll::on_frame_complete(&queue, move || feedback.record_content_frame(epoch, web_time::Instant::now()));
    }
    if probe.take_presented(consumed) {
        let feedback = feedback.clone();
        crate::device_poll::on_frame_complete(&queue, move || feedback.record_completed_frame());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracted_frame_keeps_its_generation_when_main_replaces_the_surface() {
        use bevy::{ecs::system::RunSystemOnce, prelude::World, render::MainWorld};
        let mut app = App::new();
        app.add_plugins(StartupPresentationPlugin);
        let feedback = app.world().resource::<StartupPresentation>().clone();
        feedback.replace_content_surface(true);
        let original = feedback.content_epoch();
        let mut main = MainWorld::default();
        main.insert_resource(feedback.clone());
        let mut render = World::new();
        render.insert_resource(main);
        render.init_resource::<ExtractedSurfaceEpoch>();
        render.run_system_once(extract_surface_epoch).unwrap();
        feedback.replace_content_surface(false);
        feedback.replace_content_surface(true);
        assert_eq!(render.resource::<ExtractedSurfaceEpoch>().0, original);
        feedback.record_content_frame(
            render.resource::<ExtractedSurfaceEpoch>().0.unwrap(),
            web_time::Instant::now(),
        );
        assert!(!feedback.has_presented_content());
        render.run_system_once(extract_surface_epoch).unwrap();
        feedback.record_content_frame(
            render.resource::<ExtractedSurfaceEpoch>().0.unwrap(),
            web_time::Instant::now(),
        );
        assert!(feedback.has_presented_content());
    }

    #[test]
    fn absent_unpresented_and_repeated_surfaces_never_count_as_completed_frames() {
        let mut probe = PresentationProbe::default();
        assert!(!probe.take_presented(true));
        probe.surface = Some(Entity::from_bits(1));
        assert!(!probe.take_presented(false));
        assert!(!probe.take_presented(true));
        for _ in 0..StartupPresentation::REQUIRED_FRAMES {
            probe.surface = Some(Entity::from_bits(1));
            assert!(probe.take_presented(true));
        }
        probe.surface = Some(Entity::from_bits(1));
        assert!(!probe.take_presented(true));
    }
}
