//! Particle quads from Cinnabar's camera-facing instances (`ParticleGpuFrame`), expanded as
//! `particles.wesl` does: corners `centre ± axis_x ± axis_y`, texture coordinates into the
//! particle atlas with `v` running down the quad, the instance's gamma tint, and its packed
//! block/sky light (full-bright when the particle is unlit).

use super::vertices::{FULL_BRIGHT, Mesh, Vertex};
use bevy::math::{Vec3, Vec4Swizzles};
use particles::ParticleInstance;

/// Java's raw lightmap coordinates of an instance: `center_light.w` packs block light in its
/// low nibble and sky light in the next, read when `axis_y.w` marks the particle lit.
pub(super) fn instance_light(instance: &ParticleInstance) -> [f32; 2] {
    if instance.axis_y[3] <= 0.5 {
        return FULL_BRIGHT;
    }
    let packed = instance.center_light[3].max(0.0) as u32;
    [
        (packed & 15) as f32 * 16.0,
        ((packed >> 4) & 15) as f32 * 16.0,
    ]
}

/// Splits a quad axis where sampling reaches a source-image edge, preserving interior UV slopes.
fn clamped_axis(start: f32, delta: f32, bounds: [f32; 2]) -> Vec<(f32, f32)> {
    let mut points = vec![0.0, 1.0];
    if delta != 0.0 {
        for edge in bounds {
            let t = (edge - start) / delta;
            if t > 0.0 && t < 1.0 {
                points.push(t);
            }
        }
    }
    points.sort_by(f32::total_cmp);
    points.dedup();
    points
        .into_iter()
        .map(|t| (t, (start + t * delta).clamp(bounds[0], bounds[1])))
        .collect()
}

/// Appends camera-relative quads, extending each source image's edge texels outside its UV bounds.
pub(super) fn particle_quads(
    mesh: &mut Mesh,
    instances: &[ParticleInstance],
    camera: Vec3,
    atlas_side: u32,
) {
    for instance in instances {
        let [u0, v0, du, dv] = instance.uv;
        let bounds = instance.texture_bounds;
        if !instance
            .uv
            .into_iter()
            .chain(bounds)
            .chain([
                u0 + du,
                v0 + dv,
                bounds[0] + bounds[2],
                bounds[1] + bounds[3],
            ])
            .all(f32::is_finite)
        {
            continue;
        }
        let centre = bevy::math::Vec4::from_array(instance.center_light).xyz() - camera;
        let x = Vec3::from_slice(&instance.axis_x[..3]);
        let y = Vec3::from_slice(&instance.axis_y[..3]);
        let light = instance_light(instance);
        let normal = x.cross(y);
        let half = 0.5 / atlas_side.max(1) as f32;
        let bounded = bounds[2] > 0.0 && bounds[3] > 0.0;
        let axis = |start, delta, index: usize| {
            if bounded {
                let inset = half.min(bounds[index + 2] * 0.5);
                clamped_axis(
                    start,
                    delta,
                    [
                        bounds[index] + inset,
                        bounds[index] + bounds[index + 2] - inset,
                    ],
                )
            } else {
                vec![(0.0, start), (1.0, start + delta)]
            }
        };
        let us = axis(u0, du, 0);
        let vs = axis(v0, dv, 1);
        let mid_uv = [
            (u0 + du * 0.5).clamp(
                us.first().unwrap().1.min(us.last().unwrap().1),
                us.first().unwrap().1.max(us.last().unwrap().1),
            ),
            (v0 + dv * 0.5).clamp(
                vs.first().unwrap().1.min(vs.last().unwrap().1),
                vs.first().unwrap().1.max(vs.last().unwrap().1),
            ),
        ];
        for u in us.windows(2) {
            for v in vs.windows(2) {
                mesh.quad(
                    [(u[0], v[1]), (u[1], v[1]), (u[1], v[0]), (u[0], v[0])].map(
                        |((tx, u), (ty, v))| Vertex {
                            position: centre + x * (tx * 2.0 - 1.0) + y * (1.0 - ty * 2.0),
                            colour: instance.color,
                            normal,
                            uv: [u, v],
                            mid_uv,
                            light,
                        },
                    ),
                );
            }
        }
    }
}
