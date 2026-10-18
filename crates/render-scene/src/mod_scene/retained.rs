//! Trusted same-frame anchors and compact, camera-relative instance uploads.
use bevy::math::DVec3;
use bevy::{prelude::*, render::extract_resource::ExtractResource};
use mod_render::retained::{Anchor, Cap, Instance, Join, WidthMode};
use std::collections::{BTreeMap, BTreeSet};

/// Availability is host-owned and contains no block contents or guest-visible observations.
#[derive(Resource, Clone, Debug, Default, PartialEq, ExtractResource)]
#[extract_app(bevy::render::RenderApp)]
pub struct DrawingFrame {
    pub connection: u64,
    pub dimension: i32,
    pub live_owners: BTreeSet<(u64, u64)>,
    pub entity_poses: BTreeMap<mod_api::drawing::DrawingHandle, RootPose>,
    pub loaded_blocks: BTreeSet<[i32; 3]>,
}

/// The final body root matrix from the same publication used by actor drawing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RootPose {
    pub world_from_actor: [[f32; 4]; 3],
    pub frame_generation: u64,
}

impl RootPose {
    /// Selects the exact body incarnation from the already interpolated renderer payload.
    pub fn from_actor_frame(
        frame: &crate::ActorRenderFrame,
        key: mod_render::retained::EntityKey,
    ) -> Option<Self> {
        frame
            .rig
            .instances
            .iter()
            .zip(frame.rig.manifest.iter())
            .find_map(|(instance, manifest)| {
                let id = manifest.identity;
                (id.layer == render_model::ACTOR_LAYER_BODY
                    && id.session_id == key.session
                    && id.dimension == key.dimension
                    && id.runtime_id == key.runtime_id
                    && id.spawn_revision == key.spawn_revision)
                    .then_some(Self {
                        world_from_actor: instance.world_from_actor,
                        frame_generation: frame.rig.frame_generation,
                    })
            })
    }
}

/// Model transform is already relative to the view origin before conversion to GPU floats.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GpuInstance {
    pub transform: [[f32; 4]; 4],
    pub tint: [f32; 4],
    pub line: [f32; 4],
    pub params: [f32; mod_api::materials::PARAMS],
}

/// Resolves admitted world, block and camera anchors from one trusted frame and view.
pub fn resolve(
    instance: &Instance,
    frame: &DrawingFrame,
    camera: &GlobalTransform,
) -> Option<GpuInstance> {
    if frame.connection == 0 {
        return None;
    }
    let view = camera.to_matrix();
    let camera_origin = view.w_axis.truncate().as_dvec3();
    let anchor = match instance.anchor {
        Anchor::World { dimension, origin } if dimension == frame.dimension => {
            Mat4::from_translation((DVec3::from_array(origin) - camera_origin).as_vec3())
        }
        Anchor::Block {
            dimension,
            position,
        } if dimension == frame.dimension && frame.loaded_blocks.contains(&position) => {
            Mat4::from_translation(
                (DVec3::new(position[0] as f64, position[1] as f64, position[2] as f64)
                    - camera_origin)
                    .as_vec3(),
            )
        }
        Anchor::Camera => Mat4::from_cols(view.x_axis, view.y_axis, view.z_axis, Vec4::W),
        Anchor::Entity(anchor) => {
            let pose = frame.entity_poses.get(&anchor.entity)?;
            let rows = pose.world_from_actor;
            if pose.frame_generation == 0 || rows.iter().flatten().any(|v| !v.is_finite()) {
                return None;
            }
            let origin =
                DVec3::new(rows[0][3] as f64, rows[1][3] as f64, rows[2][3] as f64) - camera_origin;
            let mut root = Mat4::from_translation(origin.as_vec3());
            if anchor.body_rotation {
                let axes: [Vec3; 3] = std::array::from_fn(|axis| {
                    Vec3::new(rows[0][axis], rows[1][axis], rows[2][axis])
                });
                root = Mat4::from_cols(
                    axes[0].try_normalize()?.extend(0.),
                    axes[1].try_normalize()?.extend(0.),
                    axes[2].try_normalize()?.extend(0.),
                    origin.as_vec3().extend(1.),
                );
            }
            root
        }
        _ => return None,
    };
    let transform = anchor
        * Mat4::from_scale_rotation_translation(
            Vec3::from_array(instance.transform.scale),
            Quat::from_array(instance.transform.rotation),
            Vec3::from_array(instance.transform.translation),
        );
    Some(GpuInstance {
        transform: transform.to_cols_array_2d(),
        tint: instance.tint,
        params: instance.params,
        line: [
            instance.width,
            if instance.width_mode == WidthMode::Pixels {
                1.
            } else {
                0.
            },
            match instance.join {
                Join::Bevel => 0.,
                Join::Miter => 1.,
                Join::Round => 2.,
            },
            if instance.cap == Cap::Round { 1. } else { 0. },
        ],
    })
}

/// Conservatively culls triangle bounds. Lines retain their viewport-dependent expansion.
pub fn visible(
    geometry: &mod_render::retained::Geometry,
    instance: &GpuInstance,
    camera: &GlobalTransform,
    projection: Mat4,
) -> bool {
    if geometry.is_polyline() {
        return true;
    }
    let view = camera.to_matrix();
    let rotation = Mat4::from_cols(view.x_axis, view.y_axis, view.z_axis, Vec4::W).inverse();
    let clip = projection * rotation * Mat4::from_cols_array_2d(&instance.transform);
    let bounds = geometry.bounds();
    let corners: [Vec4; 8] = std::array::from_fn(|index| {
        clip * Vec3::new(
            bounds[index & 1][0],
            bounds[(index >> 1) & 1][1],
            bounds[(index >> 2) & 1][2],
        )
        .extend(1.)
    });
    ![0, 1, 2, 3, 4, 5].iter().any(|plane| {
        corners.iter().all(|p| match *plane {
            0 => p.x < -p.w,
            1 => p.x > p.w,
            2 => p.y < -p.w,
            3 => p.y > p.w,
            4 => p.z < 0.,
            _ => p.z > p.w,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mod_render::retained::{Blend, Depth, Transform};
    /// Supplies a bounded identity drawing for anchor-resolution probes.
    pub(super) fn instance(anchor: Anchor) -> Instance {
        Instance {
            geometry: mod_api::drawing::DrawingHandle {
                owner: 1,
                generation: 1,
                slot: 1,
            },
            material: None,
            params: mod_api::materials::Preset::default().0,
            anchor,
            transform: Transform::default(),
            tint: [1.; 4],
            depth: Depth::Test,
            blend: Blend::Premultiplied,
            width: 1.,
            width_mode: WidthMode::World,
            join: Join::Bevel,
            cap: Cap::Butt,
        }
    }
    #[test]
    fn high_precision_origin_is_subtracted_before_gpu_conversion() {
        let frame = DrawingFrame {
            connection: 1,
            ..Default::default()
        };
        let camera = GlobalTransform::from_translation(Vec3::new(29_999_998., 0., 0.));
        let gpu = resolve(
            &instance(Anchor::World {
                dimension: 0,
                origin: [29_999_998.25, 0., 0.],
            }),
            &frame,
            &camera,
        )
        .unwrap();
        assert_eq!(gpu.transform[3][0], 0.25);
    }
    #[test]
    fn block_unload_and_dimension_changes_hide_immediately() {
        let mut frame = DrawingFrame {
            connection: 1,
            ..Default::default()
        };
        let draw = instance(Anchor::Block {
            dimension: 0,
            position: [0; 3],
        });
        assert!(resolve(&draw, &frame, &GlobalTransform::IDENTITY).is_none());
        frame.loaded_blocks.insert([0; 3]);
        assert!(resolve(&draw, &frame, &GlobalTransform::IDENTITY).is_some());
        frame.dimension = 1;
        assert!(resolve(&draw, &frame, &GlobalTransform::IDENTITY).is_none());
    }
}

#[cfg(test)]
mod actor_tests {
    use super::*;
    use crate::{ActorRenderScene, ActorRenderSource};
    use mod_render::retained::{Anchor, EntityAnchor};

    /// Supplies a compatibility source whose pose publication is owned by ActorRenderScene.
    fn actor(teleported: bool) -> ActorRenderSource {
        ActorRenderSource {
            runtime_id: 7,
            unique_id: 77,
            spawn_revision: 1,
            movement_revision: 1,
            previous_position: [0.; 3],
            position: [10., 0., 0.],
            previous_pitch_degrees: 0.,
            pitch_degrees: 0.,
            previous_yaw_degrees: 0.,
            yaw_degrees: 90.,
            previous_head_yaw_degrees: 0.,
            head_yaw_degrees: 90.,
            teleported,
            skin: None,
        }
    }

    #[test]
    fn roots_use_the_actors_published_interpolation_and_teleport_snap() {
        let mut scene = ActorRenderScene::default();
        let key = mod_render::retained::EntityKey {
            session: 0,
            dimension: 0,
            runtime_id: 7,
            spawn_revision: 1,
        };
        let frame = scene.update(0.25, None, [actor(false)]);
        let root = RootPose::from_actor_frame(frame, key).unwrap();
        assert_eq!(root.world_from_actor[0][3], frame.instances[0].position[0]);
        assert_eq!(root.world_from_actor[0][3], 2.5);
        let frame = scene.update(0.25, None, [actor(true)]);
        assert_eq!(
            RootPose::from_actor_frame(frame, key)
                .unwrap()
                .world_from_actor[0][3],
            10.
        );
        assert!(
            RootPose::from_actor_frame(
                frame,
                mod_render::retained::EntityKey {
                    spawn_revision: 2,
                    ..key
                }
            )
            .is_none()
        );
    }

    #[test]
    fn missing_same_frame_roots_hide_and_body_axes_are_optional() {
        let handle = mod_api::drawing::DrawingHandle {
            owner: 1,
            generation: 1,
            slot: 1,
        };
        let mut drawing = super::tests::instance(Anchor::Entity(EntityAnchor {
            entity: handle,
            body_rotation: false,
        }));
        drawing.params = mod_api::materials::Preset::rainbow(2.).0;
        let mut frame = DrawingFrame {
            connection: 1,
            ..Default::default()
        };
        assert!(resolve(&drawing, &frame, &GlobalTransform::IDENTITY).is_none());
        frame.entity_poses.insert(
            handle,
            RootPose {
                world_from_actor: [[0., 0., 1., 5.], [0., 1., 0., 0.], [-1., 0., 0., 0.]],
                frame_generation: 1,
            },
        );
        let translation = resolve(&drawing, &frame, &GlobalTransform::IDENTITY).unwrap();
        assert_eq!(translation.transform[0], [1., 0., 0., 0.]);
        assert_eq!(translation.transform[3], [5., 0., 0., 1.]);
        assert_eq!(translation.params, drawing.params);
        drawing.anchor = Anchor::Entity(EntityAnchor {
            entity: handle,
            body_rotation: true,
        });
        assert_eq!(
            resolve(&drawing, &frame, &GlobalTransform::IDENTITY)
                .unwrap()
                .transform[0],
            [0., 0., -1., 0.]
        );
        frame.entity_poses.clear();
        assert!(resolve(&drawing, &frame, &GlobalTransform::IDENTITY).is_none());
    }
}
