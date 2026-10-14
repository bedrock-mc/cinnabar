//! Drawing distant terrain: the renderer link, the view it is selected for, the air fog
//! distance and the far clip it needs, and the renderer's counters.

use std::time::Instant;

use bevy::{
    camera::{Camera, PerspectiveProjection, Projection},
    prelude::{App, Local, Query, Res, ResMut, Resource, Transform, With},
    time::{Real, Time},
};
use client_presentation::camera::FlyCamera;
use client_runtime::distant_terrain::drawing::{
    DistantTerrainDrawing, DrawCamera, DrawFrame, render_marker_line,
};
use render_scene::distant_terrain::{
    DistantTerrainLink, DistantTerrainRenderSettings, DistantTerrainRenderStats,
};

use super::{Reported, report_changed};
use crate::runtime::world::ClientWorld;

/// The far clip covers the drawing distance this many times over, enough for the farthest
/// corner of the drawn area seen from above.
const FAR_CLIP_FACTOR: f32 = 1.5;

/// The frame's distant terrain driver.
#[derive(Resource)]
pub(crate) struct DistantTerrainDrawingResource(DistantTerrainDrawing);

impl DistantTerrainDrawingResource {
    /// True when the device cannot draw distant terrain.
    pub(crate) fn unsupported(&self) -> bool {
        self.0.unsupported()
    }
}

/// The distant terrain drawing distance in blocks while it is drawn, which air and weather
/// fog resolve against.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq)]
pub(crate) struct DistantTerrainFogDistance(pub(crate) Option<f32>);

/// Installs the renderer and registers the driver on its link.
pub(super) fn configure(app: &mut App) {
    app.add_plugins(render::DistantTerrainRenderPlugin);
    let link = app.world().resource::<DistantTerrainLink>().clone();
    app.insert_resource(DistantTerrainDrawingResource(DistantTerrainDrawing::new(
        link,
    )))
    .init_resource::<DistantTerrainFogDistance>();
}

/// The drawing choices of the Video settings; `None` while distant terrain is off.
fn render_settings(video: &ui::VideoSettings) -> Option<DistantTerrainRenderSettings> {
    video
        .distant_terrain
        .then_some(DistantTerrainRenderSettings {
            quality: video.distant_terrain_quality,
            distance_chunks: render_api::DistantTerrainTier::CURRENT
                .clamp_distance(video.distant_terrain_chunks),
        })
}

/// The camera selection measures from: its eye, viewport height and vertical field of view.
fn draw_camera(
    camera: &Camera,
    projection: &Projection,
    transform: &Transform,
) -> Option<DrawCamera> {
    let height = camera.physical_viewport_size()?.y;
    // A perspective matrix scales Y by 1 / tan(fov / 2).
    let scale = projection.get_clip_from_view().y_axis.y;
    (height > 0 && scale > 0.0).then(|| DrawCamera {
        eye: transform.translation.as_dvec3().to_array(),
        viewport_height_px: f64::from(height),
        vertical_fov_radians: 2.0 * f64::from(scale).recip().atan(),
    })
}

/// Hands the renderer this frame's settings and view through the driver, moves air fog out
/// to the drawn distance once the renderer draws there, and logs the renderer's counters
/// when they change.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_distant_terrain(
    mut drawing: ResMut<DistantTerrainDrawingResource>,
    mut client_world: ResMut<ClientWorld>,
    settings: Res<client_state::RuntimeSettings>,
    cameras: Query<(&Camera, &Projection, &Transform), With<FlyCamera>>,
    real_time: Res<Time<Real>>,
    link: Res<DistantTerrainLink>,
    mut fog: ResMut<DistantTerrainFogDistance>,
    mut reported: Local<Reported<DistantTerrainRenderStats>>,
) {
    let video = settings.user_settings_update().1.video;
    let wanted = render_settings(&video);
    let drawing = &mut drawing.0;
    if wanted.is_none() && drawing.idle() && reported_off(&reported) {
        // Off, the renderer told so and its freed state logged: nothing to hand over or log.
        return;
    }
    drawing.update(DrawFrame {
        stream: client_world.stream.as_mut(),
        settings: wanted,
        camera: cameras
            .single()
            .ok()
            .and_then(|(camera, projection, transform)| draw_camera(camera, projection, transform)),
        uptime: real_time.elapsed(),
        frame_seconds: real_time.delta_secs(),
    });
    let distance = drawing
        .drawn_distance_chunks()
        .map(|chunks| (chunks * world::SUB_CHUNK_SIDE as u32) as f32);
    if fog.0 != distance {
        fog.0 = distance;
    }
    if let Some(line) = report_changed(
        link.stats(),
        &mut reported,
        Instant::now,
        render_marker_line,
    ) {
        eprintln!("{line}");
    }
}

/// True when nothing was logged yet, or the last line logged showed the renderer with
/// nothing allocated, so an off renderer has nothing more to report.
fn reported_off(reported: &Reported<DistantTerrainRenderStats>) -> bool {
    reported.at.is_none() || (!reported.last.ready && reported.last.pool_bytes == 0)
}

/// Covers the distant terrain distance with the far clip, and restores the default when it
/// is not drawn.
pub(super) fn apply_far_clip(
    fog: Res<DistantTerrainFogDistance>,
    mut cameras: Query<&mut Projection, With<FlyCamera>>,
) {
    let default = PerspectiveProjection::default().far;
    let wanted = fog
        .0
        .map_or(default, |blocks| (blocks * FAR_CLIP_FACTOR).max(default));
    for mut projection in &mut cameras {
        if let Projection::Perspective(perspective) = projection.as_ref()
            && perspective.far != wanted
            && let Projection::Perspective(perspective) = projection.as_mut()
        {
            perspective.far = wanted;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_map_to_the_renderer_only_while_the_toggle_is_on() {
        let mut video = ui::UserSettings::default().video;
        assert_eq!(render_settings(&video), None);
        video.distant_terrain = true;
        video.distant_terrain_chunks = u32::MAX;
        video.distant_terrain_quality = render_api::DistantTerrainQuality::High;
        assert_eq!(
            render_settings(&video),
            Some(DistantTerrainRenderSettings {
                quality: render_api::DistantTerrainQuality::High,
                distance_chunks: render_api::DistantTerrainTier::CURRENT.max_distance_chunks,
            })
        );
    }

    #[test]
    fn only_a_logged_freed_renderer_counts_as_reported_off() {
        let mut reported = Reported::<DistantTerrainRenderStats>::default();
        assert!(reported_off(&reported), "never turned on");
        reported.at = Some(Instant::now());
        reported.last = DistantTerrainRenderStats {
            ready: true,
            pool_bytes: 256 << 20,
            ..Default::default()
        };
        assert!(
            !reported_off(&reported),
            "the freed state is still to be logged"
        );
        reported.last = DistantTerrainRenderStats::default();
        assert!(reported_off(&reported));
    }

    #[test]
    fn the_far_clip_covers_the_drawn_distance_and_returns_to_the_default() {
        let mut app = App::new();
        app.init_resource::<DistantTerrainFogDistance>()
            .add_systems(bevy::prelude::Update, apply_far_clip);
        let camera = app
            .world_mut()
            .spawn((
                FlyCamera::default(),
                Projection::Perspective(Default::default()),
            ))
            .id();
        let far = |app: &App| match app.world().get::<Projection>(camera) {
            Some(Projection::Perspective(perspective)) => perspective.far,
            _ => panic!("a perspective camera"),
        };
        let default = PerspectiveProjection::default().far;
        app.update();
        assert_eq!(far(&app), default);
        app.world_mut()
            .resource_mut::<DistantTerrainFogDistance>()
            .0 = Some(8192.0);
        app.update();
        assert_eq!(far(&app), 8192.0 * FAR_CLIP_FACTOR);
        app.world_mut()
            .resource_mut::<DistantTerrainFogDistance>()
            .0 = None;
        app.update();
        assert_eq!(far(&app), default);
    }
}
