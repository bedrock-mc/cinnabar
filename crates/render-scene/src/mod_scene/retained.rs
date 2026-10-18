//! Trusted same-frame anchors and compact, camera-relative instance uploads.
use bevy::math::DVec3;
use bevy::{prelude::*, render::extract_resource::ExtractResource};
use mod_render::retained::{Anchor, Cap, Instance, Join, WidthMode};
use std::collections::BTreeSet;

/// Availability is host-owned and contains no block contents or guest-visible observations.
#[derive(Resource, Clone, Debug, Default, PartialEq, ExtractResource)]
#[extract_app(bevy::render::RenderApp)]
pub struct DrawingFrame {
    pub connection: u64,
    pub dimension: i32,
    pub live_owners: BTreeSet<(u64, u64)>,
    pub loaded_blocks: BTreeSet<[i32; 3]>,
}

/// Model transform is already relative to the view origin before conversion to GPU floats.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GpuInstance {
    pub transform: [[f32; 4]; 4],
    pub tint: [f32; 4],
    pub line: [f32; 4],
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
    fn instance(anchor: Anchor) -> Instance {
        Instance {
            geometry: mod_api::drawing::DrawingHandle {
                owner: 1,
                generation: 1,
                slot: 1,
            },
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
