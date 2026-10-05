//! Portal view distortion and the hand's independent perspective.

use bevy::{
    camera::{CameraProjection, SubCameraView},
    math::Vec3A,
    prelude::*,
    window::PrimaryWindow,
};

use super::{
    CameraSettingsAuthority, FlyCamera, ServerCameraView,
    fov::{CameraFovInputs, CameraFovState},
    projection_fov_radians, window_aspect,
};

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
    let base = settings.horizontal_fov_degrees();
    let fov_degrees = server.fov_override_degrees(base).unwrap_or(base * modifier);
    for mut projection in &mut cameras {
        let perspective = match projection.as_mut() {
            Projection::Perspective(perspective) => Some(perspective),
            Projection::Custom(custom) => custom
                .get_mut::<PortalProjection>()
                .map(|portal| &mut portal.perspective),
            Projection::Orthographic(_) => None,
        };
        if let Some(perspective) = perspective {
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
}
