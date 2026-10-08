//! Turns live particles into camera-facing GPU instances: culling, tinting, lighting, sorting.

use bytemuck::{Pod, Zeroable};

use super::{
    def::{Billboard, DirectionMode, EffectDef, Facing, Material, TextureSource, Tint},
    emitter::{Emitter, transform},
    molang::{Program, Queries, Rng},
    system::ParticleSystem,
    world::ParticleWorld,
};

/// One particle quad. `axis_*` are half-extent edge vectors; `axis_x.w` is 1 for
/// alpha-tested opaque particles; `center_light.w` packs block/sky nibbles,
/// and `axis_y.w` enables the shared world lightmap.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct ParticleInstance {
    pub center_light: [f32; 4],
    pub axis_x: [f32; 4],
    pub axis_y: [f32; 4],
    /// Atlas `u0, v0, du, dv`.
    pub uv: [f32; 4],
    /// Native gamma RGB tint plus alpha.
    pub color: [f32; 4],
}

/// Camera basis the draw list is built against.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParticleView {
    pub position: [f32; 3],
    pub right: [f32; 3],
    pub up: [f32; 3],
    pub forward: [f32; 3],
    /// Half of the frustum's diagonal angle in radians.
    pub half_diagonal: f32,
}

#[derive(Clone, Debug, Default)]
pub struct DrawLists {
    /// Alpha, blend and opaque materials, sorted far to near.
    pub blend: Vec<ParticleInstance>,
    pub add: Vec<ParticleInstance>,
}

pub const MAX_DRAW_DISTANCE: f32 = 128.0;

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn unit(v: [f32; 3]) -> Option<[f32; 3]> {
    let length = dot(v, v).sqrt();
    (length > 1e-6).then(|| v.map(|c| c / length))
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// Screen-plane axes for a facing mode; `direction` is the particle's travel direction.
fn basis_for(
    facing: Facing,
    view: &ParticleView,
    emitter_basis: &[[f32; 3]; 3],
    position: [f32; 3],
    direction: Option<[f32; 3]>,
) -> ([f32; 3], [f32; 3]) {
    let to_camera = unit(sub(view.position, position)).unwrap_or(view.forward.map(|c| -c));
    let world_up = [0.0, 1.0, 0.0];
    let facing_axes = |up: [f32; 3]| {
        let right = unit(cross(up, to_camera)).unwrap_or(view.right);
        (right, cross(to_camera, right))
    };
    match facing {
        Facing::RotateXyz => (view.right, view.up),
        Facing::LookatXyz => facing_axes(view.up),
        Facing::RotateY => {
            let right = unit([view.right[0], 0.0, view.right[2]]).unwrap_or([1.0, 0.0, 0.0]);
            (right, world_up)
        }
        Facing::LookatY => {
            let flat = [to_camera[0], 0.0, to_camera[2]];
            let right = unit(cross(world_up, flat)).unwrap_or(view.right);
            (right, world_up)
        }
        Facing::LookatDirection | Facing::DirectionY => match direction {
            Some(up) => {
                let right = unit(cross(up, to_camera)).unwrap_or(view.right);
                (right, up)
            }
            None => facing_axes(view.up),
        },
        Facing::DirectionX => match direction {
            Some(right) => {
                let up = unit(cross(to_camera, right)).unwrap_or(view.up);
                (right, up)
            }
            None => facing_axes(view.up),
        },
        Facing::DirectionZ => match direction {
            Some(normal) => {
                let right = unit(cross(world_up, normal)).unwrap_or(view.right);
                (right, cross(normal, right))
            }
            None => facing_axes(view.up),
        },
        Facing::EmitterTransformXy => (emitter_basis[0], emitter_basis[1]),
        Facing::EmitterTransformXz => (emitter_basis[0], emitter_basis[2]),
        Facing::EmitterTransformYz => (emitter_basis[1], emitter_basis[2]),
    }
}

fn eval_color(
    programs: &[Program; 4],
    vars: &mut Vec<f32>,
    rng: &mut Rng,
    queries: &Queries,
) -> [f32; 4] {
    std::array::from_fn(|i| programs[i].eval(vars, rng, queries))
}

fn tint_of(tint: &Tint, vars: &mut Vec<f32>, rng: &mut Rng, queries: &Queries) -> [f32; 4] {
    match tint {
        Tint::White => [1.0; 4],
        Tint::Fixed(color) => eval_color(color, vars, rng, queries),
        Tint::Gradient { stops, interpolant } => {
            let t = interpolant.eval(vars, rng, queries);
            let next = stops.partition_point(|stop| stop.0 <= t);
            if next == 0 {
                return eval_color(&stops[0].1, vars, rng, queries);
            }
            if next >= stops.len() {
                return eval_color(&stops[stops.len() - 1].1, vars, rng, queries);
            }
            let (low, high) = (&stops[next - 1], &stops[next]);
            let span = (high.0 - low.0).max(1e-6);
            let f = ((t - low.0) / span).clamp(0.0, 1.0);
            let a = eval_color(&low.1, vars, rng, queries);
            let b = eval_color(&high.1, vars, rng, queries);
            std::array::from_fn(|i| a[i] + (b[i] - a[i]) * f)
        }
    }
}

/// Atlas-normalized `(u0, v0, du, dv)` for the current flipbook frame.
fn uv_of(
    billboard: &Billboard,
    def: &EffectDef,
    placement: [f32; 4],
    age_fraction: (f32, f32),
    vars: &mut Vec<f32>,
    rng: &mut Rng,
    queries: &Queries,
) -> [f32; 4] {
    let uv = &billboard.uv;
    let mut origin = [
        uv.uv[0].eval(vars, rng, queries),
        uv.uv[1].eval(vars, rng, queries),
    ];
    let mut extent = [
        uv.size[0].eval(vars, rng, queries),
        uv.size[1].eval(vars, rng, queries),
    ];
    if let Some(flipbook) = &uv.flipbook {
        let base = [
            flipbook.base[0].eval(vars, rng, queries),
            flipbook.base[1].eval(vars, rng, queries),
        ];
        let step = [
            flipbook.step[0].eval(vars, rng, queries),
            flipbook.step[1].eval(vars, rng, queries),
        ];
        extent = [
            flipbook.size[0].eval(vars, rng, queries),
            flipbook.size[1].eval(vars, rng, queries),
        ];
        let frames = flipbook.max_frame.eval(vars, rng, queries).max(1.0).floor();
        let raw = if flipbook.stretch_to_lifetime {
            age_fraction.0 / age_fraction.1.max(1e-6) * frames
        } else {
            age_fraction.0 * flipbook.fps.eval(vars, rng, queries)
        };
        let mut frame = raw.floor().max(0.0);
        frame = if flipbook.looping {
            frame % frames
        } else {
            frame.min(frames - 1.0)
        };
        origin = [base[0] + step[0] * frame, base[1] + step[1] * frame];
    }
    if matches!(def.texture, TextureSource::Path(_)) {
        let [tw, th] = uv.texture_size;
        let [pu, pv, pw, ph] = placement;
        [
            pu + origin[0] / tw * pw,
            pv + origin[1] / th * ph,
            extent[0] / tw * pw,
            extent[1] / th * ph,
        ]
    } else {
        [origin[0], origin[1], extent[0], extent[1]]
    }
}

fn light_sample(world: &dyn ParticleWorld, cell: [i32; 3]) -> f32 {
    let (block, sky) = world.light(cell);
    f32::from(block.min(15) | (sky.min(15) << 4))
}

fn emit_emitter(
    emitter: &mut Emitter,
    view: &ParticleView,
    world: &dyn ParticleWorld,
    cos_limit: f32,
    out: &mut Vec<(f32, ParticleInstance, bool)>,
) {
    let def = std::sync::Arc::clone(&emitter.def);
    let local = def.emitter.local_position;
    let placement = emitter.texture.normalized();
    let (origin, basis, queries) = (emitter.pos, emitter.basis, emitter.queries);
    let additive = def.material == Material::Add;
    let opaque = if def.material == Material::Opaque {
        1.0
    } else {
        0.0
    };
    let rng = &mut emitter.rng;
    for p in &mut emitter.particles {
        let position: [f32; 3] = if local {
            let rotated = transform(&basis, p.pos);
            std::array::from_fn(|i| origin[i] + rotated[i])
        } else {
            p.pos
        };
        let relative = sub(position, view.position);
        let distance_sq = dot(relative, relative);
        if distance_sq > MAX_DRAW_DISTANCE * MAX_DRAW_DISTANCE {
            continue;
        }
        if distance_sq > 4.0 && dot(relative, view.forward) < cos_limit * distance_sq.sqrt() {
            continue;
        }
        if let Some(per_render) = &def.particle.per_render {
            per_render.eval(&mut p.vars, rng, &queries);
        }
        let size = [
            def.particle.billboard.size[0].eval(&mut p.vars, rng, &queries),
            def.particle.billboard.size[1].eval(&mut p.vars, rng, &queries),
        ];
        if !(size[0].abs() > 1e-5 && size[1].abs() > 1e-5) {
            continue;
        }
        let direction = match &def.particle.billboard.direction {
            DirectionMode::DeriveFromVelocity => {
                let velocity = if def.emitter.local_velocity {
                    transform(&basis, p.vel)
                } else {
                    p.vel
                };
                let speed_sq = dot(velocity, velocity);
                let threshold = def.particle.billboard.min_speed_threshold;
                (speed_sq > threshold * threshold)
                    .then(|| unit(velocity))
                    .flatten()
            }
            DirectionMode::Custom(vector) => unit([
                vector[0].eval(&mut p.vars, rng, &queries),
                vector[1].eval(&mut p.vars, rng, &queries),
                vector[2].eval(&mut p.vars, rng, &queries),
            ]),
        };
        let (mut right, mut up) = basis_for(
            def.particle.billboard.facing,
            view,
            &basis,
            position,
            direction,
        );
        if p.rotation != 0.0
            && !matches!(
                def.particle.billboard.facing,
                Facing::DirectionX | Facing::DirectionY | Facing::DirectionZ
            )
        {
            let (sin, cos) = p.rotation.to_radians().sin_cos();
            let (r, u) = (right, up);
            right = std::array::from_fn(|i| r[i] * cos + u[i] * sin);
            up = std::array::from_fn(|i| u[i] * cos - r[i] * sin);
        }
        let mut color = tint_of(&def.particle.tint, &mut p.vars, rng, &queries);
        let uv = uv_of(
            &def.particle.billboard,
            &def,
            placement,
            (p.age, p.lifetime),
            &mut p.vars,
            rng,
            &queries,
        );
        let sample = if def.particle.lit {
            light_sample(world, position.map(|c| c.floor() as i32))
        } else {
            0.0
        };
        color[3] = color[3].clamp(0.0, 1.0);
        if color[3] <= 0.0 {
            continue;
        }
        let instance = ParticleInstance {
            center_light: [position[0], position[1], position[2], sample],
            axis_x: [
                right[0] * size[0],
                right[1] * size[0],
                right[2] * size[0],
                opaque,
            ],
            axis_y: [
                up[0] * size[1],
                up[1] * size[1],
                up[2] * size[1],
                f32::from(def.particle.lit),
            ],
            uv,
            color: [
                color[0].clamp(0.0, 1.0),
                color[1].clamp(0.0, 1.0),
                color[2].clamp(0.0, 1.0),
                color[3],
            ],
        };
        out.push((distance_sq, instance, additive));
    }
}

impl ParticleSystem {
    /// Builds the frame's draw lists against `view`, evaluating per-render expressions.
    pub fn build_draw(&mut self, view: &ParticleView, world: &dyn ParticleWorld) -> DrawLists {
        let cos_limit = (view.half_diagonal + 0.15).min(3.0).cos();
        let mut collected = Vec::new();
        for emitter in self.emitters_mut() {
            emit_emitter(emitter, view, world, cos_limit, &mut collected);
        }
        collected.sort_unstable_by(|a, b| b.0.total_cmp(&a.0));
        let mut lists = DrawLists::default();
        for (_, instance, additive) in collected {
            if additive {
                lists.add.push(instance);
            } else {
                lists.blend.push(instance);
            }
        }
        lists
    }
}

const _: () = assert!(std::mem::size_of::<ParticleInstance>() == 80);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{emitter::SpawnRequest, world::EmptyWorld};

    fn view() -> ParticleView {
        ParticleView {
            position: [0.0, 0.0, 0.0],
            right: [1.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            forward: [0.0, 0.0, -1.0],
            half_diagonal: 1.0,
        }
    }

    const EFFECT: &str = r#"{"particle_effect":{"description":{"identifier":"minecraft:quad","basic_render_parameters":{"material":"particles_alpha","texture":"textures/none"}},
      "components":{
        "minecraft:emitter_lifetime_once":{"active_time":1},
        "minecraft:emitter_rate_instant":{"num_particles":1},
        "minecraft:emitter_shape_point":{},
        "minecraft:particle_lifetime_expression":{"max_lifetime":2},
        "minecraft:particle_appearance_billboard":{"size":[0.5,0.25],"facing_camera_mode":"rotate_xyz","uv":{"texture_width":16,"texture_height":16,"uv":[8,0],"uv_size":[8,8]}},
        "minecraft:particle_appearance_tinting":{"color":[1,0.5,0,0.5]}}}}"#;

    fn system_with_particle(z: f32) -> ParticleSystem {
        let mut system = ParticleSystem::default();
        assert!(system.register_effect(EFFECT.as_bytes()));
        system.spawn(&SpawnRequest {
            effect: "minecraft:quad".into(),
            position: [0.0, 0.0, z],
            ..SpawnRequest::default()
        });
        system.tick(0.02, &EmptyWorld);
        system
    }

    #[test]
    fn instance_carries_size_uv_and_tint() {
        let mut system = system_with_particle(-5.0);
        let lists = system.build_draw(&view(), &EmptyWorld);
        assert_eq!(lists.blend.len(), 1);
        let instance = lists.blend[0];
        assert_eq!(instance.axis_x[0], 0.5);
        assert_eq!(instance.axis_y[1], 0.25);
        assert_eq!(instance.color[3], 0.5);
        assert_eq!(instance.color[..3], [1.0, 0.5, 0.0]);
        assert_eq!(
            instance.axis_y[3], 0.0,
            "unlit effect bypasses the lightmap"
        );
        // Missing texture falls back to the 1x1 white texel: u origin is half of a 1px placement.
        assert!(instance.uv[2] > 0.0);
    }

    #[test]
    fn lit_particles_publish_both_light_channels_without_a_brightness_floor() {
        struct CellLight(u8, u8);
        impl ParticleWorld for CellLight {
            fn solid_boxes(&self, _: [f32; 3], _: [f32; 3], _: &mut Vec<[f32; 6]>) {}
            fn light(&self, cell: [i32; 3]) -> (u8, u8) {
                assert_eq!(cell, [0, 0, -5]);
                (self.0, self.1)
            }
            fn fluid(&self, _: [i32; 3]) -> super::super::world::Fluid {
                super::super::world::Fluid::None
            }
        }
        let effect = EFFECT.replace(
            "\"minecraft:particle_appearance_tinting\"",
            "\"minecraft:particle_appearance_lighting\":{},\"minecraft:particle_appearance_tinting\"",
        );
        let mut system = ParticleSystem::default();
        assert!(system.register_effect(effect.as_bytes()));
        system.spawn(&SpawnRequest {
            effect: "minecraft:quad".into(),
            position: [0.0, 0.0, -5.0],
            ..SpawnRequest::default()
        });
        system.tick(0.02, &EmptyWorld);
        for (block, sky) in [(0, 0), (0, 15), (10, 0), (7, 12), (255, 255)] {
            let instance = system.build_draw(&view(), &CellLight(block, sky)).blend[0];
            assert_eq!(
                instance.center_light[3],
                f32::from(block.min(15) | (sky.min(15) << 4))
            );
            assert_eq!(instance.axis_y[3], 1.0);
        }
    }

    #[test]
    fn particles_behind_the_camera_are_culled() {
        let mut system = system_with_particle(8.0);
        assert!(system.build_draw(&view(), &EmptyWorld).blend.is_empty());
    }

    #[test]
    fn far_particles_are_culled() {
        let mut system = system_with_particle(-100.0);
        let mut far = view();
        far.position = [0.0, 0.0, 100.0];
        assert!(system.build_draw(&far, &EmptyWorld).blend.is_empty());
    }

    #[test]
    fn lookat_basis_is_orthonormal_toward_the_camera() {
        let (right, up) = basis_for(
            Facing::LookatXyz,
            &view(),
            &[[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            [3.0, 1.0, -4.0],
            None,
        );
        assert!(dot(right, up).abs() < 1e-5);
        assert!((dot(right, right) - 1.0).abs() < 1e-4);
    }
}
