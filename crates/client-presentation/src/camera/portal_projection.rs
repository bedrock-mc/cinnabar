//! Portal view distortion and the hand's independent perspective.

#[cfg(test)]
use bevy::prelude::{App, Entity, OrthographicProjection, Transform, Update, World};
use bevy::{
    camera::{CameraProjection, SubCameraView},
    math::Vec3A,
    prelude::{
        Mat4, PerspectiveProjection, Projection, Query, Res, ResMut, Single, Time, Vec3, Window,
        With,
    },
    window::PrimaryWindow,
};

use super::{
    CameraSettingsAuthority, FlyCamera, ServerCameraView,
    fov::{CameraFovInputs, CameraFovState},
    projection_fov_radians, window_aspect,
};

const MIN_GAMEPLAY_FOV_DEGREES: f32 = 5.0;
const MAX_GAMEPLAY_FOV_DEGREES: f32 = 130.0;

/// Native rotates an X-axis scale around (0, 1, 1), then reverses the rotation.
/// The distortion setting changes the scale; it does not fade the portal texture.
pub(super) fn portal_distortion(
    progress: f32,
    elapsed_ticks: f32,
    confusion: bool,
    distortion_scale: f32,
) -> Mat4 {
    if !progress.is_finite() || progress <= 0.0 || !elapsed_ticks.is_finite() {
        return Mat4::IDENTITY;
    }
    let scale = if distortion_scale.is_finite() {
        distortion_scale.clamp(0.0, 1.0)
    } else {
        1.0
    };
    if scale == 0.0 {
        return Mat4::IDENTITY;
    }
    let p = progress.clamp(0.0, 1.0);
    let skew = (-0.04 * p + 5.0 / (p * p + 5.0)).powi(2);
    let x_scale = 1.0 / (1.0 + scale * (skew - 1.0));
    let phase = elapsed_ticks * if confusion { 7.0 } else { 20.0 };
    let rotation = Mat4::from_axis_angle(Vec3::new(0.0, 1.0, 1.0).normalize(), phase.to_radians());
    rotation * Mat4::from_scale(Vec3::new(x_scale, 1.0, 1.0)) * rotation.transpose()
}

/// Carries the non-rigid view distortion in the projection, including visibility frusta.
#[derive(Clone, Debug)]
pub(super) struct PortalProjection {
    pub perspective: PerspectiveProjection,
    pub distortion: Mat4,
}

/// The hand retains its independent perspective while the world camera is distorted.
pub fn first_person_hand_fov(projection: &Projection) -> Option<f32> {
    let perspective = match projection {
        Projection::Perspective(_) => true,
        Projection::Custom(custom) => custom.get::<PortalProjection>().is_some(),
        Projection::Orthographic(_) => false,
    };
    perspective.then(|| crate::actor_publication::HAND_FOV_DEGREES.to_radians())
}

/// Updates a world perspective's FOV without replacing its portal distortion or aspect.
pub fn set_camera_projection_fov(projection: &mut Projection, degrees: f32) {
    if let Some(perspective) = perspective_mut(projection) {
        perspective.fov = projection_fov_radians(degrees);
    }
}

/// Returns the world perspective, including the one carried by portal distortion.
fn perspective_mut(projection: &mut Projection) -> Option<&mut PerspectiveProjection> {
    match projection {
        Projection::Perspective(perspective) => Some(perspective),
        Projection::Custom(custom) => custom
            .get_mut::<PortalProjection>()
            .map(|portal| &mut portal.perspective),
        Projection::Orthographic(_) => None,
    }
}

impl CameraProjection for PortalProjection {
    fn get_clip_from_view(&self) -> Mat4 {
        self.perspective.get_clip_from_view() * self.distortion
    }

    fn get_clip_from_view_for_sub(&self, sub_view: &SubCameraView) -> Mat4 {
        self.perspective.get_clip_from_view_for_sub(sub_view) * self.distortion
    }

    fn update(&mut self, width: f32, height: f32) {
        self.perspective.update(width, height);
    }

    fn far(&self) -> f32 {
        self.perspective.far
    }

    fn get_frustum_corners(&self, z_near: f32, z_far: f32) -> [Vec3A; 8] {
        let inverse = self.distortion.inverse();
        self.perspective
            .get_frustum_corners(z_near, z_far)
            .map(|corner| inverse.transform_point3a(corner))
    }
}

pub(super) fn apply_distortion(projection: &mut Projection, distortion: Mat4) {
    match projection {
        Projection::Perspective(perspective) if distortion != Mat4::IDENTITY => {
            *projection = Projection::custom(PortalProjection {
                perspective: perspective.clone(),
                distortion,
            });
        }
        Projection::Custom(custom) => {
            if let Some(portal) = custom.get_mut::<PortalProjection>() {
                if distortion == Mat4::IDENTITY {
                    *projection = Projection::Perspective(portal.perspective.clone());
                } else {
                    portal.distortion = distortion;
                }
            }
        }
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
pub fn update_camera_fov(
    window: Single<&Window, With<PrimaryWindow>>,
    settings: Res<CameraSettingsAuthority>,
    time: Res<Time>,
    inputs: Res<CameraFovInputs>,
    mut fov_state: ResMut<CameraFovState>,
    server: Res<ServerCameraView>,
    mut cameras: Query<&mut Projection, With<FlyCamera>>,
) {
    let modifier = fov_state.advance(inputs.target_modifier(), time.delta_secs());
    let modifier = if server.gameplay_fov_enabled() {
        modifier
    } else {
        1.0
    };
    let base = settings.horizontal_fov_degrees();
    let gameplay_fov = (base * modifier).clamp(MIN_GAMEPLAY_FOV_DEGREES, MAX_GAMEPLAY_FOV_DEGREES);
    let rig_delta = settings.rig().map_or(0.0, |rig| rig.fov_delta_degrees);
    let mut player_fov = gameplay_fov + rig_delta;
    if server.player_effects_enabled() && inputs.death_ticks.is_some() {
        player_fov = inputs
            .death_fov_degrees(player_fov, !server.has_pose_override())
            .clamp(MIN_GAMEPLAY_FOV_DEGREES, MAX_GAMEPLAY_FOV_DEGREES);
    }
    let fov_degrees =
        server.fov_override_degrees(base).unwrap_or(player_fov) * settings.fov_scale();
    for mut projection in &mut cameras {
        if let Some(perspective) = perspective_mut(&mut projection) {
            perspective.fov = projection_fov_radians(fov_degrees);
            perspective.aspect_ratio = window_aspect(&window);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn hand_frame() -> render::ActorRigRenderFrame {
        let builder = render::ActorRigFrameBuilder::new([]).unwrap();
        render::ActorRigRenderFrame {
            frame_generation: 1,
            geometry_revision: 1,
            instances: Arc::from([render::ActorGpuInstance::default()]),
            previous_bones: Arc::from([[[0.0; 4]; 3]]),
            current_bones: Arc::from([[[0.0; 4]; 3]]),
            geometry_vertices: builder.geometry_vertices().clone(),
            geometry_spans: Arc::from([render::ActorRigGeometrySpan {
                first_vertex: 0,
                vertex_count: 1,
            }]),
            maximum_vertex_count: 1,
            ..Default::default()
        }
    }

    #[test]
    fn portal_camera_keeps_the_animated_hand_with_its_independent_fov() {
        let mut projection = Projection::Perspective(PerspectiveProjection {
            fov: 110.0_f32.to_radians(),
            ..Default::default()
        });
        let original_world_projection = projection.get_clip_from_view();
        let hand_fov = crate::actor_publication::HAND_FOV_DEGREES.to_radians();
        let skin = protocol::SkinRgba8::from(Arc::<[u8]>::from(vec![
            255;
            render_model::STANDARD_SKIN_BYTES
        ]));
        let mut scene = render::HandRigScene::default();
        let light = render::HandRigLight {
            block_level: 15,
            sky_level: 0,
            daylight: 1.0,
            pad: 0,
            ..Default::default()
        };
        for (index, distortion) in [
            Mat4::IDENTITY,
            portal_distortion(0.25, 3.0, false, 1.0),
            portal_distortion(1.0, 17.0, false, 1.0),
            Mat4::IDENTITY,
        ]
        .into_iter()
        .enumerate()
        {
            apply_distortion(&mut projection, distortion);
            assert_eq!(first_person_hand_fov(&projection), Some(hand_fov));
            assert_eq!(
                projection.get_clip_from_view() == original_world_projection,
                distortion == Mat4::IDENTITY
            );
            assert!(scene.publish(
                hand_frame(),
                skin.clone(),
                light,
                first_person_hand_fov(&projection).unwrap(),
                index as u64 + 1,
            ));
            assert!(
                scene.is_active(),
                "portal phase {index} keeps the rig active"
            );
        }
    }

    #[test]
    fn hand_projection_admission_rejects_orthographic_and_unrelated_custom_cameras() {
        assert_eq!(
            first_person_hand_fov(&Projection::Orthographic(
                OrthographicProjection::default_3d()
            )),
            None
        );
        assert_eq!(
            first_person_hand_fov(&Projection::custom(PerspectiveProjection::default())),
            None
        );
    }

    #[test]
    fn native_portal_distortion_scales_x_at_zero_phase() {
        let matrix = portal_distortion(1.0, 0.0, false, 1.0);
        let skew = (-0.04_f32 + 5.0 / 6.0).powi(2);
        assert!((matrix.x_axis.x - skew.recip()).abs() < 1e-6);
        assert_eq!(matrix.y_axis.y, 1.0);
        assert_eq!(matrix.z_axis.z, 1.0);
        assert_eq!(portal_distortion(1.0, 5.0, false, 0.0), Mat4::IDENTITY);
    }

    #[test]
    fn rotating_native_scale_preserves_volume_and_changes_confusion_speed() {
        let portal = portal_distortion(0.5, 2.0, false, 1.0);
        let confusion = portal_distortion(0.5, 2.0, true, 1.0);
        assert_ne!(portal, confusion);
        assert!((portal.determinant() - confusion.determinant()).abs() < 1e-6);
        assert!((portal - portal.transpose()).abs_diff_eq(Mat4::ZERO, 1e-6));
    }

    #[test]
    fn custom_projection_restores_the_original_perspective() {
        let mut projection = Projection::default();
        let original = projection.get_clip_from_view();
        apply_distortion(&mut projection, portal_distortion(1.0, 4.0, false, 1.0));
        assert!(matches!(projection, Projection::Custom(_)));
        assert_ne!(projection.get_clip_from_view(), original);
        apply_distortion(&mut projection, Mat4::IDENTITY);
        assert!(matches!(projection, Projection::Perspective(_)));
        assert_eq!(projection.get_clip_from_view(), original);
    }

    #[test]
    fn explicit_fov_preserves_portal_distortion_and_the_window_aspect() {
        let mut projection = Projection::Perspective(PerspectiveProjection {
            aspect_ratio: 2.0,
            ..Default::default()
        });
        let distortion = portal_distortion(1.0, 4.0, false, 1.0);
        apply_distortion(&mut projection, distortion);
        set_camera_projection_fov(&mut projection, 40.0);
        let Projection::Custom(custom) = &projection else {
            panic!("portal distortion must remain active");
        };
        let portal = custom.get::<PortalProjection>().unwrap();
        assert_eq!(portal.distortion, distortion);
        assert_eq!(portal.perspective.aspect_ratio, 2.0);
        assert!((portal.perspective.fov.to_degrees() - 40.0).abs() < 0.001);
        assert_eq!(
            first_person_hand_fov(&projection),
            Some(crate::actor_publication::HAND_FOV_DEGREES.to_radians())
        );
    }

    fn fov_app(degrees: f32, inputs: CameraFovInputs) -> (App, Entity) {
        let mut settings = ui::UserSettings::default();
        settings.video.horizontal_fov_degrees = degrees;
        let mut authority = CameraSettingsAuthority::default();
        authority.replace(1, &settings).unwrap();
        let mut app = App::new();
        app.init_resource::<Time>()
            .insert_resource(authority)
            .insert_resource(inputs)
            .init_resource::<CameraFovState>()
            .init_resource::<ServerCameraView>()
            .add_systems(Update, update_camera_fov);
        app.world_mut().spawn((Window::default(), PrimaryWindow));
        let camera = app
            .world_mut()
            .spawn((FlyCamera::default(), Projection::default()))
            .id();
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs(1));
        (app, camera)
    }

    fn world_fov(app: &App, camera: Entity) -> f32 {
        match app.world().get::<Projection>(camera).unwrap() {
            Projection::Perspective(projection) => projection.fov,
            Projection::Custom(custom) => custom.get::<PortalProjection>().unwrap().perspective.fov,
            Projection::Orthographic(_) => panic!("perspective expected"),
        }
    }

    #[test]
    fn death_fov_updates_the_live_portal_projection_and_clears_on_recovery() {
        let (mut app, camera) = fov_app(
            100.0,
            CameraFovInputs {
                death_ticks: Some(120.0),
                ..Default::default()
            },
        );
        apply_distortion(
            app.world_mut()
                .get_mut::<Projection>(camera)
                .unwrap()
                .as_mut(),
            portal_distortion(1.0, 4.0, false, 1.0),
        );
        app.update();
        assert!((world_fov(&app, camera).to_degrees() - 84.0).abs() < 1e-4);
        app.world_mut()
            .resource_mut::<CameraFovInputs>()
            .death_ticks = None;
        app.update();
        assert!((world_fov(&app, camera).to_degrees() - 110.0).abs() < 1e-3);
        assert!(matches!(
            app.world().get::<Projection>(camera).unwrap(),
            Projection::Custom(_)
        ));
    }

    #[test]
    fn death_fov_preserves_explicit_server_fov_overrides() {
        use protocol::{CameraEvent, CameraFovInstruction, CameraInstructionEvent};
        let (mut app, camera) = fov_app(
            100.0,
            CameraFovInputs {
                death_ticks: Some(120.0),
                ..Default::default()
            },
        );
        app.world_mut().resource_mut::<ServerCameraView>().apply(
            1,
            &CameraEvent::Instruction(Box::new(CameraInstructionEvent {
                fov: Some(CameraFovInstruction {
                    degrees: 50.0,
                    ease_time_seconds: 0.0,
                    ease_type: Arc::from("linear"),
                    clear: false,
                }),
                ..Default::default()
            })),
            &super::super::ViewContext {
                base: Transform::IDENTITY,
                subject: Transform::IDENTITY,
                base_fov: 100.0,
                actors: &|_| None,
            },
        );
        app.update();
        assert!((world_fov(&app, camera).to_degrees() - 50.0).abs() < 1e-4);
    }

    #[test]
    fn death_fov_respects_a_free_cameras_player_effects_selector() {
        use protocol::{CameraEvent, CameraInstructionEvent, CameraPreset, CameraSetInstruction};
        for (player_effects, expected) in [(None, 100.0), (Some(true), 50.0)] {
            let (mut app, camera) = fov_app(
                100.0,
                CameraFovInputs {
                    death_ticks: Some(500.0),
                    ..Default::default()
                },
            );
            let context = super::super::ViewContext {
                base: Transform::IDENTITY,
                subject: Transform::IDENTITY,
                base_fov: 100.0,
                actors: &|_| None,
            };
            let mut server = app.world_mut().resource_mut::<ServerCameraView>();
            server.apply(
                1,
                &CameraEvent::Presets(
                    vec![CameraPreset {
                        name: "free_death_effects".into(),
                        inherit_from: "minecraft:free".into(),
                        player_effects,
                        ..Default::default()
                    }]
                    .into(),
                ),
                &context,
            );
            server.apply(
                2,
                &CameraEvent::Instruction(Box::new(CameraInstructionEvent {
                    set: Some(CameraSetInstruction {
                        preset_id: 0,
                        ease: None,
                        position: None,
                        rotation_degrees: None,
                        facing_position: None,
                        view_offset: None,
                        entity_offset: None,
                        default_preset: None,
                        remove_ignore_starting_values: false,
                    }),
                    ..Default::default()
                })),
                &context,
            );
            app.update();
            assert!((world_fov(&app, camera).to_degrees() - expected).abs() < 1e-4);
        }
    }

    #[test]
    fn current_view_scale_changes_projection_and_restores_latest_saved_fov() {
        let (mut app, camera) = fov_app(
            80.0,
            CameraFovInputs {
                fov_effects_scale: 0.0,
                ..Default::default()
            },
        );
        app.world_mut()
            .resource_mut::<CameraSettingsAuthority>()
            .set_view_scale(0.25, 0.25);
        app.update();
        assert!((world_fov(&app, camera).to_degrees() - 20.0).abs() < 1e-4);
        assert_eq!(
            app.world()
                .resource::<CameraSettingsAuthority>()
                .perspective(),
            semantic_input::PerspectiveMode::FirstPerson
        );
        let mut user = ui::UserSettings::default();
        user.video.horizontal_fov_degrees = 100.0;
        app.world_mut()
            .resource_mut::<CameraSettingsAuthority>()
            .replace(2, &user)
            .unwrap();
        app.update();
        assert!((world_fov(&app, camera).to_degrees() - 25.0).abs() < 1e-4);
        app.world_mut()
            .resource_mut::<CameraSettingsAuthority>()
            .set_view_scale(1.0, 1.0);
        app.update();
        assert!((world_fov(&app, camera).to_degrees() - 100.0).abs() < 1e-4);
    }

    #[test]
    fn gameplay_fov_stays_bounded_after_large_speed_and_slowness_changes() {
        for (degrees, inputs, expected) in [
            (
                100.0,
                CameraFovInputs {
                    movement_speed: 0.3,
                    ..Default::default()
                },
                130.0_f32,
            ),
            (
                30.0,
                CameraFovInputs {
                    slowness_amplifier: Some(20),
                    ..Default::default()
                },
                5.0_f32,
            ),
        ] {
            let (mut app, camera) = fov_app(degrees, inputs);
            app.update();
            let projection = app.world().get::<Projection>(camera).unwrap();
            assert!((world_fov(&app, camera) - expected.to_radians()).abs() < 1e-6);
            assert!(
                (projection.get_clip_from_view().y_axis.y
                    - (expected.to_radians() * 0.5).tan().recip())
                .abs()
                    < 1e-5
            );
            assert_eq!(
                first_person_hand_fov(projection),
                Some(crate::actor_publication::HAND_FOV_DEGREES.to_radians())
            );
        }
    }

    #[test]
    fn gameplay_fov_bound_preserves_custom_projection_and_restores_normal_speed() {
        let (mut app, camera) = fov_app(
            100.0,
            CameraFovInputs {
                movement_speed: 0.3,
                ..Default::default()
            },
        );
        apply_distortion(
            app.world_mut()
                .get_mut::<Projection>(camera)
                .unwrap()
                .as_mut(),
            portal_distortion(1.0, 4.0, false, 1.0),
        );
        app.update();
        assert!((world_fov(&app, camera).to_degrees() - 130.0).abs() < 1e-4);
        *app.world_mut().resource_mut::<CameraFovInputs>() = CameraFovInputs::default();
        app.update();
        assert!((world_fov(&app, camera).to_degrees() - 110.0).abs() < 1e-3);
        assert!(matches!(
            app.world().get::<Projection>(camera).unwrap(),
            Projection::Custom(_)
        ));
    }

    #[test]
    fn gameplay_fov_bound_keeps_authored_rig_and_server_overrides() {
        use crate::camera::ViewContext;
        use protocol::{CameraEvent, CameraFovInstruction, CameraInstructionEvent};
        let (mut app, camera) = fov_app(
            100.0,
            CameraFovInputs {
                movement_speed: 0.3,
                ..Default::default()
            },
        );
        app.world_mut()
            .resource_mut::<CameraSettingsAuthority>()
            .set_rig(Some(super::super::CameraRig {
                offset: Vec3::ZERO,
                roll_radians: 0.0,
                fov_delta_degrees: 15.0,
            }));
        app.update();
        assert!((world_fov(&app, camera).to_degrees() - 145.0).abs() < 1e-4);
        app.world_mut().resource_mut::<ServerCameraView>().apply(
            1,
            &CameraEvent::Instruction(Box::new(CameraInstructionEvent {
                fov: Some(CameraFovInstruction {
                    degrees: 50.0,
                    ease_time_seconds: 0.0,
                    ease_type: Arc::from("linear"),
                    clear: false,
                }),
                ..Default::default()
            })),
            &ViewContext {
                base: Transform::IDENTITY,
                subject: Transform::IDENTITY,
                base_fov: 100.0,
                actors: &|_| None,
            },
        );
        app.update();
        assert!((world_fov(&app, camera).to_degrees() - 50.0).abs() < 1e-4);
    }

    #[test]
    fn free_camera_suppresses_gameplay_fov_and_clear_restores_the_retained_modifier() {
        use crate::camera::server_view::ViewContext;
        use protocol::{CameraEvent, CameraInstructionEvent, CameraPreset, CameraSetInstruction};
        let mut app = App::new();
        app.init_resource::<Time>()
            .init_resource::<CameraSettingsAuthority>()
            .init_resource::<CameraFovInputs>()
            .init_resource::<CameraFovState>()
            .init_resource::<ServerCameraView>()
            .add_systems(Update, update_camera_fov);
        app.world_mut().spawn((Window::default(), PrimaryWindow));
        let camera = app
            .world_mut()
            .spawn((FlyCamera::default(), Projection::default()))
            .id();
        app.world_mut()
            .resource_mut::<CameraFovInputs>()
            .movement_speed = 0.13;
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs(1));
        app.update();
        let base = app
            .world()
            .resource::<CameraSettingsAuthority>()
            .horizontal_fov_degrees();
        let fov = |world: &World| match world.get::<Projection>(camera).unwrap() {
            Projection::Perspective(projection) => projection.fov,
            _ => panic!("perspective expected"),
        };
        assert!(fov(app.world()) > base.to_radians());
        let context = ViewContext {
            base: Transform::IDENTITY,
            subject: Transform::IDENTITY,
            base_fov: base,
            actors: &|_| None,
        };
        {
            let mut server = app.world_mut().resource_mut::<ServerCameraView>();
            server.apply(
                1,
                &CameraEvent::Presets(
                    vec![CameraPreset {
                        name: "free_effects".into(),
                        inherit_from: "minecraft:free".into(),
                        player_effects: Some(true),
                        ..Default::default()
                    }]
                    .into(),
                ),
                &context,
            );
            server.apply(
                2,
                &CameraEvent::Instruction(Box::new(CameraInstructionEvent {
                    set: Some(CameraSetInstruction {
                        preset_id: 0,
                        ease: None,
                        position: None,
                        rotation_degrees: None,
                        facing_position: None,
                        view_offset: None,
                        entity_offset: None,
                        default_preset: None,
                        remove_ignore_starting_values: false,
                    }),
                    ..Default::default()
                })),
                &context,
            );
        }
        app.update();
        assert_eq!(fov(app.world()), base.to_radians());
        app.world_mut().resource_mut::<ServerCameraView>().clear();
        app.update();
        assert!(fov(app.world()) > base.to_radians());
    }
}
