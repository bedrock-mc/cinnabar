//! Per-particle state and its per-tick update: curves, motion, collision, kill conditions.

use std::sync::Arc;

use super::{
    def::{Curve, CurveKind, Motion},
    emitter::{Emitter, Outputs, transform},
    molang::{Program, Queries, Rng, V_PARTICLE_AGE},
    world::{Fluid, ParticleWorld},
};

pub struct Particle {
    /// World position, or emitter-local when the effect uses local-space positions.
    pub pos: [f32; 3],
    pub prev: [f32; 3],
    pub vel: [f32; 3],
    pub age: f32,
    pub lifetime: f32,
    /// Degrees.
    pub rotation: f32,
    pub rotation_rate: f32,
    pub vars: Vec<f32>,
    pub plane_side: f32,
    pub timeline_next: usize,
    pub travelled: f32,
    pub travel_next: usize,
    /// Times each looping travel event has fired.
    pub loop_counts: Vec<u32>,
}

impl Particle {
    #[must_use]
    pub fn new(pos: [f32; 3], vel: [f32; 3], lifetime: f32, vars: Vec<f32>) -> Self {
        Self {
            pos,
            prev: pos,
            vel,
            age: 0.0,
            lifetime,
            rotation: 0.0,
            rotation_rate: 0.0,
            vars,
            plane_side: 0.0,
            timeline_next: 0,
            travelled: 0.0,
            travel_next: 0,
            loop_counts: Vec::new(),
        }
    }
}

const CONTACT_EPSILON: f32 = 1e-4;

fn set_var(vars: &mut Vec<f32>, slot: u16, value: f32) {
    let slot = slot as usize;
    if slot >= vars.len() {
        vars.resize(slot + 1, f32::NAN);
    }
    vars[slot] = value;
}

pub(super) fn eval3(
    programs: &[Program; 3],
    vars: &mut Vec<f32>,
    rng: &mut Rng,
    queries: &Queries,
) -> [f32; 3] {
    [
        programs[0].eval(vars, rng, queries),
        programs[1].eval(vars, rng, queries),
        programs[2].eval(vars, rng, queries),
    ]
}

/// Evaluates a definition curve at normalized position `t`.
pub(super) fn curve_value(curve: &Curve, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let nodes = &curve.nodes;
    match curve.kind {
        CurveKind::Linear => {
            if nodes.len() < 2 {
                return nodes.first().copied().unwrap_or(0.0);
            }
            let x = t * (nodes.len() - 1) as f32;
            let i = (x.floor() as usize).min(nodes.len() - 2);
            let f = x - i as f32;
            nodes[i] + (nodes[i + 1] - nodes[i]) * f
        }
        CurveKind::Bezier => {
            if nodes.len() < 4 {
                return nodes.first().copied().unwrap_or(0.0);
            }
            let u = 1.0 - t;
            u * u * u * nodes[0]
                + 3.0 * u * u * t * nodes[1]
                + 3.0 * u * t * t * nodes[2]
                + t * t * t * nodes[3]
        }
        CurveKind::CatmullRom => {
            if nodes.len() < 4 {
                return nodes.first().copied().unwrap_or(0.0);
            }
            let x = t * (nodes.len() - 3) as f32;
            let i = (x.floor() as usize).min(nodes.len() - 4);
            let f = x - i as f32;
            let (p0, p1, p2, p3) = (nodes[i], nodes[i + 1], nodes[i + 2], nodes[i + 3]);
            0.5 * (2.0 * p1
                + (p2 - p0) * f
                + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * f * f
                + (3.0 * p1 - p0 - 3.0 * p2 + p3) * f * f * f)
        }
        CurveKind::BezierChain => {
            let chain = &curve.chain;
            let (Some(first), Some(last)) = (chain.first(), chain.last()) else {
                return 0.0;
            };
            if t <= first.0 {
                return first.1;
            }
            if t >= last.0 {
                return last.1;
            }
            let i = chain.partition_point(|node| node.0 <= t).saturating_sub(1);
            let ((t0, v0, s0), (t1, v1, s1)) = (chain[i], chain[(i + 1).min(chain.len() - 1)]);
            let dt = (t1 - t0).max(1e-6);
            let f = (t - t0) / dt;
            let (f2, f3) = (f * f, f * f * f);
            (2.0 * f3 - 3.0 * f2 + 1.0) * v0
                + (f3 - 2.0 * f2 + f) * dt * s0
                + (-2.0 * f3 + 3.0 * f2) * v1
                + (f3 - f2) * dt * s1
        }
    }
}

pub(super) fn eval_curves(curves: &[Curve], vars: &mut Vec<f32>, rng: &mut Rng, queries: &Queries) {
    for curve in curves {
        let range = curve.range.eval(vars, rng, queries);
        let input = curve.input.eval(vars, rng, queries);
        let t = if range.abs() < 1e-6 {
            0.0
        } else {
            input / range
        };
        set_var(vars, curve.slot, curve_value(curve, t));
    }
}

/// Dynamic motion integrates velocity before advancing position.
fn integrate(velocity: f32, acceleration: f32, drag: f32, dt: f32) -> (f32, f32) {
    let velocity = if drag.abs() <= f32::EPSILON {
        velocity + acceleration * dt
    } else {
        let growth = (drag * dt).exp();
        if growth >= f32::MAX {
            0.0
        } else {
            let terminal = acceleration / drag;
            (terminal * growth + (velocity - terminal)) / growth
        }
    };
    (velocity, velocity * dt)
}

fn overlaps(boxes: &[[f32; 6]], c: [f32; 3], r: f32) -> bool {
    boxes.iter().any(|b| {
        c[0] + r > b[0] + CONTACT_EPSILON
            && c[0] - r < b[3] - CONTACT_EPSILON
            && c[1] + r > b[1] + CONTACT_EPSILON
            && c[1] - r < b[4] - CONTACT_EPSILON
            && c[2] + r > b[2] + CONTACT_EPSILON
            && c[2] - r < b[5] - CONTACT_EPSILON
    })
}

/// Sweeps `prev -> pos` axis by axis against solid boxes; returns the impact speed on contact.
fn resolve_collision(
    particle: &mut Particle,
    radius: f32,
    restitution: f32,
    world: &dyn ParticleWorld,
    boxes: &mut Vec<[f32; 6]>,
) -> Option<f32> {
    boxes.clear();
    let min = std::array::from_fn(|i| particle.prev[i].min(particle.pos[i]) - radius);
    let max = std::array::from_fn(|i| particle.prev[i].max(particle.pos[i]) + radius);
    world.solid_boxes(min, max, boxes);
    if boxes.is_empty() || overlaps(boxes, particle.prev, radius) {
        return None;
    }
    let motion: [f32; 3] = std::array::from_fn(|i| particle.pos[i] - particle.prev[i]);
    let mut current = particle.prev;
    let mut impact: Option<f32> = None;
    for axis in [1usize, 0, 2] {
        let mut candidate = current;
        candidate[axis] += motion[axis];
        let mut limit = candidate[axis];
        for b in boxes.iter() {
            let across = (0..3).filter(|&j| j != axis).all(|j| {
                candidate[j] + radius > b[j] + CONTACT_EPSILON
                    && candidate[j] - radius < b[j + 3] - CONTACT_EPSILON
            });
            if !across {
                continue;
            }
            if motion[axis] > 0.0 {
                let face = b[axis] - radius;
                if face < limit && face >= current[axis] - CONTACT_EPSILON {
                    limit = face;
                }
            } else if motion[axis] < 0.0 {
                let face = b[axis + 3] + radius;
                if face > limit && face <= current[axis] + CONTACT_EPSILON {
                    limit = face;
                }
            }
        }
        if limit != candidate[axis] {
            candidate[axis] = limit;
            let speed = particle.vel[axis].abs();
            impact = Some(impact.map_or(speed, |best| best.max(speed)));
            particle.vel[axis] = -particle.vel[axis] * restitution;
        }
        current = candidate;
    }
    particle.pos = current;
    impact
}

fn cell(position: [f32; 3]) -> [i32; 3] {
    position.map(|c| c.floor() as i32)
}

fn fluid_allows(names: &[Box<str>], fluid: Fluid) -> bool {
    let wants = |needle: &str| names.iter().any(|name| name.contains(needle));
    let named_fluid = wants("water") || wants("bubble_column") || wants("lava");
    if !named_fluid {
        return true;
    }
    (fluid == Fluid::Water && (wants("water") || wants("bubble_column")))
        || (fluid == Fluid::Lava && wants("lava"))
}

impl Emitter {
    /// Advances every particle by `dt` seconds, firing events into `output`.
    pub fn update_particles(
        &mut self,
        dt: f32,
        world: &dyn ParticleWorld,
        boxes: &mut Vec<[f32; 6]>,
        output: &mut Outputs,
    ) {
        let def = Arc::clone(&self.def);
        let local = def.emitter.local_position;
        self.queries.is_in_water = f32::from(world.fluid(cell(self.pos)) == Fluid::Water);
        let (origin, basis, queries) = (self.pos, self.basis, self.queries);
        let mut particles = std::mem::take(&mut self.particles);
        let mut pending: Vec<(Box<str>, [f32; 3], [f32; 3])> = Vec::new();
        let rng = &mut self.rng;
        particles.retain_mut(|p| {
            p.age += dt;
            set_var(&mut p.vars, V_PARTICLE_AGE, p.age);
            eval_curves(&def.curves, &mut p.vars, rng, &queries);
            if let Some(update) = &def.particle.per_update {
                update.eval(&mut p.vars, rng, &queries);
            }
            p.prev = p.pos;
            match &def.particle.motion {
                Motion::Dynamic {
                    acceleration,
                    drag,
                    rotation_acceleration,
                    rotation_drag,
                } => {
                    let acc = eval3(acceleration, &mut p.vars, rng, &queries);
                    let drag = drag.eval(&mut p.vars, rng, &queries);
                    let mut shifts = [0.0f32; 3];
                    for (axis, acceleration) in acc.into_iter().enumerate() {
                        let (velocity, shift) = integrate(p.vel[axis], acceleration, drag, dt);
                        p.vel[axis] = velocity;
                        shifts[axis] = shift;
                    }
                    if def.emitter.local_velocity && !local {
                        shifts = transform(&basis, shifts);
                    }
                    for (component, shift) in p.pos.iter_mut().zip(shifts) {
                        *component += shift;
                    }
                    let spin_acc = rotation_acceleration.eval(&mut p.vars, rng, &queries);
                    let spin_drag = rotation_drag.eval(&mut p.vars, rng, &queries);
                    let rate = p.rotation_rate + (spin_acc - spin_drag * p.rotation_rate) * dt;
                    p.rotation_rate = rate;
                    p.rotation += rate * dt;
                }
                Motion::Parametric { position, rotation } => {
                    let relative = eval3(position, &mut p.vars, rng, &queries);
                    p.pos = if local {
                        relative
                    } else {
                        let rotated = transform(&basis, relative);
                        std::array::from_fn(|i| origin[i] + rotated[i])
                    };
                    p.rotation = rotation.eval(&mut p.vars, rng, &queries);
                }
                Motion::None => {}
            }
            let mut alive = p.age < p.lifetime;
            if let Some(collision) = def.particle.collision.as_ref().filter(|_| !local)
                && collision.enabled.eval(&mut p.vars, rng, &queries) != 0.0
                && !matches!(def.particle.motion, Motion::Parametric { .. })
            {
                let radius = collision.radius.eval(&mut p.vars, rng, &queries).max(0.0);
                let restitution = collision
                    .restitution
                    .eval(&mut p.vars, rng, &queries)
                    .clamp(0.0, 1.0);
                if let Some(speed) = resolve_collision(p, radius, restitution, world, boxes) {
                    let drag = collision.drag.eval(&mut p.vars, rng, &queries).max(0.0);
                    let damp = (1.0 - drag * dt).max(0.0);
                    p.vel = p.vel.map(|c| c * damp);
                    for event in &collision.events {
                        if speed >= event.min_speed {
                            pending.push((event.event.clone(), p.pos, p.vel));
                        }
                    }
                    if collision.expire_on_contact {
                        alive = false;
                    }
                }
            }
            let world_pos: [f32; 3] = if local {
                let rotated = transform(&basis, p.pos);
                std::array::from_fn(|i| origin[i] + rotated[i])
            } else {
                p.pos
            };
            if let Some(plane) = def.particle.kill_plane {
                let rel: [f32; 3] = if local {
                    p.pos
                } else {
                    std::array::from_fn(|i| p.pos[i] - origin[i])
                };
                let side = plane[0] * rel[0] + plane[1] * rel[1] + plane[2] * rel[2] + plane[3];
                if p.plane_side * side < 0.0 {
                    alive = false;
                }
            }
            if alive
                && (!def.particle.expire_if_not_in.is_empty()
                    || !def.particle.expire_if_in.is_empty())
            {
                let fluid = world.fluid(cell(world_pos));
                if !fluid_allows(&def.particle.expire_if_not_in, fluid) {
                    alive = false;
                }
                if !def.particle.expire_if_in.is_empty()
                    && fluid != Fluid::None
                    && fluid_allows(&def.particle.expire_if_in, fluid)
                    && def
                        .particle
                        .expire_if_in
                        .iter()
                        .any(|name| name.contains("water") || name.contains("lava"))
                {
                    alive = false;
                }
            }
            p.travelled += (0..3)
                .map(|i| (p.pos[i] - p.prev[i]).powi(2))
                .sum::<f32>()
                .sqrt();
            while let Some((distance, event)) = def.particle.travel_events.get(p.travel_next) {
                if *distance > p.travelled {
                    break;
                }
                pending.push((event.clone(), world_pos, p.vel));
                p.travel_next += 1;
            }
            if !def.particle.looping_travel_events.is_empty() {
                p.loop_counts
                    .resize(def.particle.looping_travel_events.len(), 0);
                for (index, (interval, event)) in
                    def.particle.looping_travel_events.iter().enumerate()
                {
                    if *interval <= 1e-4 {
                        continue;
                    }
                    let due = ((p.travelled / interval) as u32).min(p.loop_counts[index] + 8);
                    while p.loop_counts[index] < due {
                        pending.push((event.clone(), world_pos, p.vel));
                        p.loop_counts[index] += 1;
                    }
                }
            }
            while let Some((time, event)) = def.particle.timeline.get(p.timeline_next) {
                if *time > p.age {
                    break;
                }
                pending.push((event.clone(), world_pos, p.vel));
                p.timeline_next += 1;
            }
            if !alive {
                for event in &def.particle.expiration_events {
                    pending.push((event.clone(), world_pos, p.vel));
                }
            }
            alive
        });
        self.particles = particles;
        for (event, position, velocity) in pending {
            self.run_event(&event, position, velocity, output);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        atlas::ParticleAtlas, def::parse_effect, emitter::SpawnRequest, world::EmptyWorld,
    };

    struct Floor;

    impl ParticleWorld for Floor {
        fn solid_boxes(&self, _min: [f32; 3], _max: [f32; 3], out: &mut Vec<[f32; 6]>) {
            out.push([-100.0, -1.0, -100.0, 100.0, 0.0, 100.0]);
        }
        fn light(&self, _block: [i32; 3]) -> (u8, u8) {
            (0, 15)
        }
        fn fluid(&self, _block: [i32; 3]) -> Fluid {
            Fluid::None
        }
    }

    fn falling(collision: &str) -> Emitter {
        let json = format!(
            r#"{{"particle_effect":{{"description":{{"identifier":"t","basic_render_parameters":{{"material":"particles_alpha","texture":"x"}}}},
          "components":{{
            "minecraft:emitter_lifetime_once":{{"active_time":1}},
            "minecraft:emitter_rate_instant":{{"num_particles":1}},
            "minecraft:emitter_shape_point":{{"offset":[0,2,0],"direction":[0,0,0]}},
            "minecraft:particle_lifetime_expression":{{"max_lifetime":5}},
            "minecraft:particle_motion_dynamic":{{"linear_acceleration":[0,-10,0]}},
            {collision}
            "minecraft:particle_appearance_billboard":{{"size":[0.1,0.1],"facing_camera_mode":"lookat_xyz","uv":{{"uv":[0,0],"uv_size":[1,1]}}}}}}}}}}"#
        );
        let def = Arc::new(parse_effect(json.as_bytes()).unwrap());
        let mut emitter = Emitter::new(
            1,
            def,
            ParticleAtlas::default().fallback(),
            &SpawnRequest::default(),
            1,
        );
        emitter.advance(0.001, &mut Outputs::default(), 100);
        emitter
    }

    #[test]
    fn particles_rest_on_the_floor_and_do_not_fall_through() {
        let mut emitter = falling(
            r#""minecraft:particle_motion_collision":{"collision_radius":0.1,"coefficient_of_restitution":0},"#,
        );
        let mut boxes = Vec::new();
        for _ in 0..240 {
            emitter.update_particles(1.0 / 60.0, &Floor, &mut boxes, &mut Outputs::default());
        }
        let y = emitter.particles[0].pos[1];
        assert!((y - 0.1).abs() < 1e-3, "{y}");
    }

    #[test]
    fn without_collision_particles_fall_freely() {
        let mut emitter = falling("");
        let mut boxes = Vec::new();
        for _ in 0..120 {
            emitter.update_particles(1.0 / 60.0, &EmptyWorld, &mut boxes, &mut Outputs::default());
        }
        assert!(emitter.particles[0].pos[1] < -10.0);
    }

    #[test]
    fn expire_on_contact_removes_the_particle() {
        let mut emitter = falling(
            r#""minecraft:particle_motion_collision":{"collision_radius":0.1,"expire_on_contact":true},"#,
        );
        let mut boxes = Vec::new();
        for _ in 0..240 {
            emitter.update_particles(1.0 / 60.0, &Floor, &mut boxes, &mut Outputs::default());
        }
        assert!(emitter.particles.is_empty());
    }

    #[test]
    fn linear_curve_interpolates_evenly() {
        let curve = Curve {
            slot: 0,
            kind: CurveKind::Linear,
            nodes: vec![0.0, 10.0, 0.0],
            chain: Vec::new(),
            input: Program::constant(0.0),
            range: Program::constant(1.0),
        };
        assert_eq!(curve_value(&curve, 0.25), 5.0);
        assert_eq!(curve_value(&curve, 0.5), 10.0);
        assert_eq!(curve_value(&curve, 1.0), 0.0);
    }

    #[test]
    fn drag_integration_approaches_terminal_velocity() {
        let (velocity, _) = integrate(0.0, -10.0, 5.0, 10.0);
        assert!((velocity + 2.0).abs() < 1e-3);
    }

    #[test]
    fn dynamic_acceleration_moves_with_the_updated_velocity() {
        let (velocity, shift) = integrate(1.0, -1.5, 0.0, 0.05);
        assert!((velocity - 0.925).abs() < 1e-6);
        assert!((shift - 0.04625).abs() < 1e-6);
        let mut particle = Particle::new([0.0; 3], [1.0, 0.0, 0.0], 1.0, Vec::new());
        for _ in 0..20 {
            let (velocity, shift) = integrate(particle.vel[0], -1.5, 0.0, 0.05);
            particle.vel[0] = velocity;
            particle.pos[0] += shift;
        }
        assert!((particle.pos[0] - 0.2125).abs() < 1e-5);
        assert!((particle.vel[0] + 0.5).abs() < 1e-5);
    }

    #[test]
    fn dynamic_drag_uses_the_updated_velocity_and_native_overflow_retirement() {
        let (velocity, shift) = integrate(2.0, 0.0, 1.0, 0.5);
        let expected_velocity = 2.0 * (-0.5_f32).exp();
        assert!((velocity - expected_velocity).abs() < 1e-6);
        assert!((shift - expected_velocity * 0.5).abs() < 1e-6);
        assert_eq!(integrate(2.0, 1.5, 1000.0, 1.0), (0.0, 0.0));
    }

    #[test]
    fn dynamic_spin_updates_angular_rate_before_angle_with_euler_drag() {
        let mut emitter = falling("");
        let Motion::Dynamic {
            rotation_acceleration,
            rotation_drag,
            ..
        } = &mut Arc::get_mut(&mut emitter.def).unwrap().particle.motion
        else {
            panic!("dynamic motion fixture");
        };
        *rotation_acceleration = Program::constant(3.0);
        *rotation_drag = Program::constant(2.0);
        emitter.particles[0].rotation_rate = 4.0;
        emitter.update_particles(0.25, &EmptyWorld, &mut Vec::new(), &mut Outputs::default());
        assert_eq!(emitter.particles[0].rotation_rate, 2.75);
        assert_eq!(emitter.particles[0].rotation, 0.6875);
    }
    #[test]
    fn review_render_collision_detects_a_segment_crossing_the_whole_floor() {
        let mut particle = Particle::new([0.0, -2.0, 0.0], [0.0, -4.0, 0.0], 1.0, Vec::new());
        particle.prev = [0.0, 2.0, 0.0];
        assert_eq!(
            resolve_collision(&mut particle, 0.1, 0.0, &Floor, &mut Vec::new()),
            Some(4.0)
        );
        assert!((particle.pos[1] - 0.1).abs() < 1e-6);
        assert_eq!(particle.vel[1], 0.0);
    }
}
