//! Publishes camera-only exposure alongside the resolved camera pose.

use bevy::{prelude::*, render::render_resource::TextureUsages};
use render::motion_blur::CameraMotionBlur;
use semantic_input::PerspectiveMode;

use super::{CameraRig, CameraSettingsAuthority, FlyCamera, ServerCameraView};
use crate::local_player::LocalViewPose;

#[derive(Clone, Copy, PartialEq)]
struct CameraAnchor {
    player: u64,
    server: u64,
    perspective: PerspectiveMode,
    rig: Option<CameraRig>,
}

/// Tracks the camera anchors consumed by the rendered pose, independently of gameplay prediction.
#[derive(Default)]
pub struct MotionBlurHistory {
    previous: Option<CameraAnchor>,
    epoch: u64,
}

/// Only the world camera receives exposure; viewmodel and UI cameras keep their own unfiltered passes.
pub fn apply_camera_motion_blur(
    mut commands: Commands,
    settings: Res<CameraSettingsAuthority>,
    view: Res<LocalViewPose>,
    server: Res<ServerCameraView>,
    time: Option<Res<Time<Real>>>,
    mut history: Local<MotionBlurHistory>,
    mut cameras: Query<(Entity, &mut Camera3d, Option<&mut CameraMotionBlur>), With<FlyCamera>>,
) {
    let quality = settings.motion_blur();
    if quality == ui::MotionBlurQuality::Off {
        history.previous = None;
        for (entity, _, blur) in &mut cameras {
            if blur.is_some() {
                commands.entity(entity).remove::<CameraMotionBlur>();
            }
        }
        return;
    }
    let anchor = CameraAnchor {
        player: view.camera_reanchor_epoch(),
        server: server.camera_reanchor_epoch(),
        perspective: settings.perspective(),
        rig: settings.rig(),
    };
    let reanchored = history.previous != Some(anchor);
    if reanchored {
        history.epoch = history.epoch.wrapping_add(1);
        history.previous = Some(anchor);
    }
    let exposure_seconds = quality.shutter_angle_degrees() / 360.0 / ui::MOTION_BLUR_REFERENCE_FPS;
    for (entity, mut camera, blur) in &mut cameras {
        let usage = TextureUsages::from(camera.depth_texture_usages);
        if !usage.contains(TextureUsages::TEXTURE_BINDING) {
            camera.depth_texture_usages = (usage | TextureUsages::TEXTURE_BINDING).into();
        }
        let desired = CameraMotionBlur {
            exposure_seconds: if reanchored || blur.is_none() {
                0.0
            } else {
                exposure_seconds
            },
            samples: quality.sample_count(),
            reset_epoch: history.epoch,
            delta_seconds: time.as_ref().map_or(0.0, |time| time.delta_secs()),
        };
        if let Some(mut blur) = blur {
            if *blur != desired {
                *blur = desired;
            }
        } else {
            commands.entity(entity).insert(desired);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn app() -> (App, Entity) {
        let mut app = App::new();
        app.init_resource::<CameraSettingsAuthority>()
            .init_resource::<LocalViewPose>()
            .init_resource::<ServerCameraView>()
            .init_resource::<Time<Real>>()
            .add_systems(PostUpdate, apply_camera_motion_blur);
        let entity = app
            .world_mut()
            .spawn((FlyCamera::default(), Camera3d::default()))
            .id();
        (app, entity)
    }

    fn select(app: &mut App, quality: ui::MotionBlurQuality) {
        let mut settings = ui::UserSettings::default();
        settings.video.motion_blur = quality;
        let mut authority = app.world_mut().resource_mut::<CameraSettingsAuthority>();
        let generation = authority.generation() + 1;
        authority.replace(generation, &settings).unwrap();
    }

    fn blur(app: &App, entity: Entity) -> CameraMotionBlur {
        *app.world().get::<CameraMotionBlur>(entity).unwrap()
    }

    #[test]
    fn motion_blur_defaults_off_and_attaches_only_to_the_world_camera() {
        let (mut app, world_camera) = app();
        let viewmodel = app.world_mut().spawn(Camera3d::default()).id();
        let hud = app.world_mut().spawn(Camera2d).id();
        app.update();
        assert!(app.world().get::<CameraMotionBlur>(world_camera).is_none());
        select(&mut app, ui::MotionBlurQuality::High);
        app.update();
        assert_eq!(blur(&app, world_camera).exposure_seconds, 0.0);
        app.update();
        let active = blur(&app, world_camera);
        assert_eq!(active.samples, ui::MotionBlurQuality::High.sample_count());
        assert_eq!(
            active.exposure_seconds,
            ui::MotionBlurQuality::High.shutter_angle_degrees()
                / 360.0
                / ui::MOTION_BLUR_REFERENCE_FPS
        );
        assert!(
            TextureUsages::from(
                app.world()
                    .get::<Camera3d>(world_camera)
                    .unwrap()
                    .depth_texture_usages
            )
            .contains(TextureUsages::TEXTURE_BINDING)
        );
        assert!(app.world().get::<CameraMotionBlur>(viewmodel).is_none());
        assert!(app.world().get::<CameraMotionBlur>(hud).is_none());
        select(&mut app, ui::MotionBlurQuality::Off);
        app.update();
        assert!(app.world().get::<CameraMotionBlur>(world_camera).is_none());
        select(&mut app, ui::MotionBlurQuality::Low);
        app.update();
        assert_eq!(blur(&app, world_camera).exposure_seconds, 0.0);
        assert_ne!(blur(&app, world_camera).reset_epoch, active.reset_epoch);
    }

    #[test]
    fn motion_blur_reanchor_frame_has_zero_strength_and_resumes_afterward() {
        let (mut app, camera) = app();
        select(&mut app, ui::MotionBlurQuality::Medium);
        app.update();
        app.update();
        let previous = blur(&app, camera);
        app.world_mut()
            .resource_mut::<LocalViewPose>()
            .reanchor_camera();
        app.update();
        let reanchored = blur(&app, camera);
        assert_eq!(reanchored.exposure_seconds, 0.0);
        assert_ne!(reanchored.reset_epoch, previous.reset_epoch);
        app.update();
        assert_eq!(
            blur(&app, camera).exposure_seconds,
            previous.exposure_seconds
        );
        assert_eq!(blur(&app, camera).reset_epoch, reanchored.reset_epoch);
        app.world_mut().resource_mut::<ServerCameraView>().clear();
        app.update();
        assert_eq!(blur(&app, camera).exposure_seconds, 0.0);
        assert_ne!(blur(&app, camera).reset_epoch, reanchored.reset_epoch);
    }

    #[test]
    fn motion_blur_perspective_change_resets_without_changing_the_player_pose() {
        let (mut app, camera) = app();
        select(&mut app, ui::MotionBlurQuality::Low);
        app.update();
        app.update();
        let player = *app.world().resource::<LocalViewPose>();
        app.world_mut()
            .resource_mut::<CameraSettingsAuthority>()
            .cycle_perspective();
        app.update();
        assert_eq!(blur(&app, camera).exposure_seconds, 0.0);
        assert_eq!(*app.world().resource::<LocalViewPose>(), player);
        app.update();
        assert!(blur(&app, camera).exposure_seconds > 0.0);
    }

    #[test]
    fn motion_blur_publishes_real_frame_time_and_retains_unchanged_inputs() {
        let (mut app, camera) = app();
        select(&mut app, ui::MotionBlurQuality::Medium);
        app.update();
        for frame_time in [
            Duration::from_nanos(16_666_667),
            Duration::from_nanos(4_166_667),
        ] {
            app.world_mut()
                .resource_mut::<Time<Real>>()
                .advance_by(frame_time);
            app.update();
            assert_eq!(blur(&app, camera).delta_seconds, frame_time.as_secs_f32());
        }
        app.world_mut().clear_trackers();
        app.update();
        assert!(
            !app.world()
                .entity(camera)
                .get_ref::<CameraMotionBlur>()
                .unwrap()
                .is_changed()
        );
    }
}
