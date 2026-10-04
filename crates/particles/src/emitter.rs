//! Emitter lifecycle and particle spawning.

use std::{collections::VecDeque, sync::Arc};

use super::{
    atlas::Placement,
    def::{Direction, EffectDef, EventNode, Lifetime, Rate, Shape, SpawnKind},
    molang::{
        Program, Queries, Rng, V_EMITTER_AGE, V_EMITTER_LIFETIME, V_EMITTER_RANDOM, V_PARTICLE_AGE,
        V_PARTICLE_LIFETIME, V_PARTICLE_RANDOM,
    },
    particle::Particle,
};

mod manual;

/// Pixels for one dynamic terrain tile.
#[derive(Clone, Debug)]
pub struct TileRequest {
    pub key: u64,
    pub size: u32,
    pub pixels: Arc<[u8]>,
}

/// Everything needed to start one emitter.
#[derive(Clone, Debug, Default)]
pub struct SpawnRequest {
    pub effect: String,
    pub position: [f32; 3],
    /// Independent uniform offsets around the emitter origin, sampled once at creation.
    pub position_spread: [f32; 3],
    /// Emitter x, y, z axes in world space; identity when absent.
    pub basis: Option<[[f32; 3]; 3]>,
    /// Molang variables (`variable.` prefix already stripped, lowercase).
    pub variables: Vec<(String, f32)>,
    pub queries: Queries,
    pub tile: Option<TileRequest>,
    pub inherit_velocity: Option<[f32; 3]>,
    /// Emit exactly this many particles from a manual-rate emitter.
    pub manual_count: Option<u32>,
    /// Follows an actor (`runtime_id`, offset from its feet); the host moves the emitter each
    /// frame from [`ParticleSystem::bound_emitters`].
    pub bound: Option<(u64, [f32; 3])>,
    pub depth: u8,
    pub seed: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParticleSound {
    pub name: Box<str>,
    pub position: [f32; 3],
}

#[derive(Default)]
pub struct Outputs {
    pub spawns: Vec<SpawnRequest>,
    pub sounds: Vec<ParticleSound>,
}

pub const MAX_SPAWN_DEPTH: u8 = 4;
const MAX_PARTICLES_PER_BURST: f32 = 512.0;
const MAX_EMITTER_AGE: f32 = 600.0;

pub struct Emitter {
    pub id: u64,
    pub def: Arc<EffectDef>,
    pub texture: Placement,
    pub pos: [f32; 3],
    pub basis: [[f32; 3]; 3],
    pub age: f32,
    pub vars: Vec<f32>,
    pub rng: Rng,
    pub queries: Queries,
    pub particles: VecDeque<Particle>,
    pub inherit_velocity: [f32; 3],
    pub depth: u8,
    pub done: bool,
    pub bound: Option<(u64, [f32; 3])>,
    active_time: f32,
    sleep_time: f32,
    cycle: u64,
    was_active: bool,
    accum: f32,
    manual_pending: u32,
    /// Native biome-tinted effects share a manual emitter for each effect/RGBA8 pair.
    pub(super) biome_tinted_key: Option<[u8; 4]>,
}

fn set_var(vars: &mut Vec<f32>, slot: u16, value: f32) {
    let slot = slot as usize;
    if slot >= vars.len() {
        vars.resize(slot + 1, f32::NAN);
    }
    vars[slot] = value;
}

pub(super) fn transform(basis: &[[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|i| basis[0][i] * v[0] + basis[1][i] * v[1] + basis[2][i] * v[2])
}

pub(super) fn normalized(v: [f32; 3]) -> Option<[f32; 3]> {
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    (length > 1e-6).then(|| v.map(|c| c / length))
}

impl Emitter {
    pub fn new(
        id: u64,
        def: Arc<EffectDef>,
        texture: Placement,
        request: &SpawnRequest,
        seed: u64,
    ) -> Self {
        let mut rng = Rng::new(seed);
        let mut vars = vec![f32::NAN; def.interner.len()];
        for (name, value) in &request.variables {
            if let Some(slot) = def.interner.get(name) {
                set_var(&mut vars, slot, *value);
            }
        }
        for index in 0..4u16 {
            let value = rng.unit();
            set_var(&mut vars, V_EMITTER_RANDOM + index, value);
        }
        let mut queries = request.queries;
        queries.frame_alpha = 1.0;
        if let Some(creation) = &def.emitter.creation {
            creation.eval(&mut vars, &mut rng, &queries);
        }
        let (active_time, sleep_time) = match &def.emitter.lifetime {
            Lifetime::Once { active } => (active.eval(&mut vars, &mut rng, &queries), 0.0),
            Lifetime::Looping { active, sleep } => (
                active.eval(&mut vars, &mut rng, &queries),
                sleep.eval(&mut vars, &mut rng, &queries),
            ),
            Lifetime::Expression { .. } => (f32::INFINITY, 0.0),
        };
        set_var(&mut vars, V_EMITTER_LIFETIME, active_time);
        set_var(&mut vars, V_EMITTER_AGE, 0.0);
        let pos = std::array::from_fn(|i| {
            let spread = request.position_spread[i];
            request.position[i]
                + if spread > 0.0 {
                    rng.range(-spread, spread)
                } else {
                    0.0
                }
        });
        Self {
            id,
            def,
            texture,
            pos,
            basis: request
                .basis
                .unwrap_or([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]),
            age: 0.0,
            vars,
            rng,
            queries,
            particles: VecDeque::new(),
            inherit_velocity: request.inherit_velocity.unwrap_or([0.0; 3]),
            depth: request.depth,
            done: false,
            bound: request.bound,
            active_time: active_time.max(0.0),
            sleep_time: sleep_time.max(0.0),
            cycle: u64::MAX,
            was_active: false,
            accum: 0.0,
            manual_pending: request.manual_count.unwrap_or(1),
            biome_tinted_key: None,
        }
    }

    /// True once the emitter will emit no more and every particle has died.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        let burst_spent = matches!(self.def.emitter.rate, Rate::Instant { .. })
            && self.cycle != u64::MAX
            && matches!(self.def.emitter.lifetime, Lifetime::Once { .. });
        self.particles.is_empty() && (self.done || burst_spent)
    }

    fn eval(&mut self, program: &Program) -> f32 {
        program.eval(&mut self.vars, &mut self.rng, &self.queries)
    }

    /// Advances the emitter clock, lifetime and spawning by `dt` seconds.
    pub fn advance(&mut self, dt: f32, output: &mut Outputs, live_budget: usize) {
        self.age += dt;
        set_var(&mut self.vars, V_EMITTER_AGE, self.age);
        let def = Arc::clone(&self.def);
        if let Some(per_update) = &def.emitter.per_update {
            self.eval(per_update);
        }
        let (active, cycle) = match &def.emitter.lifetime {
            Lifetime::Once { .. } => (self.age <= self.active_time, 0),
            Lifetime::Looping { .. } => {
                let period = (self.active_time + self.sleep_time).max(1e-3);
                let cycle = (self.age / period) as u64;
                (self.age - cycle as f32 * period <= self.active_time, cycle)
            }
            Lifetime::Expression {
                activation,
                expiration,
            } => {
                if self.eval(expiration) != 0.0 {
                    self.done = true;
                }
                let active = !self.done && self.eval(activation) != 0.0;
                (active, 0)
            }
        };
        if self.age > MAX_EMITTER_AGE {
            self.done = true;
        }
        let active = active && !self.done;
        let starting = active && (!self.was_active || cycle != self.cycle);
        if starting {
            self.cycle = cycle;
        }
        self.was_active = active;
        let mut to_spawn = 0.0f32;
        match &def.emitter.rate {
            Rate::Instant { count } => {
                if starting {
                    to_spawn = self.eval(count).clamp(0.0, MAX_PARTICLES_PER_BURST);
                }
            }
            Rate::Steady { per_second, max } => {
                if active {
                    let max = self.eval(max).clamp(0.0, MAX_PARTICLES_PER_BURST);
                    self.accum += self.eval(per_second).max(0.0) * dt;
                    let room = (max - self.particles.len() as f32).max(0.0);
                    to_spawn = self.accum.floor().min(room);
                    self.accum -= to_spawn;
                    self.accum = self.accum.min(1.0);
                }
            }
            Rate::Manual { max } => {
                if active {
                    let max = self.eval(max).clamp(0.0, MAX_PARTICLES_PER_BURST);
                    let room = (max - self.particles.len() as f32).max(0.0);
                    to_spawn = (self.manual_pending as f32).min(room);
                    self.manual_pending = 0;
                    // Ordinary manual bursts finish after one request. Native cached
                    // biome-tinted emitters remain available for later block origins.
                    if self.biome_tinted_key.is_none() {
                        self.done = true;
                    }
                }
            }
        }
        let count = (to_spawn as usize).min(live_budget.max(1));
        for _ in 0..count {
            self.spawn_particle(output);
        }
        if self.age > self.active_time
            && !matches!(def.emitter.lifetime, Lifetime::Looping { .. })
            && !matches!(def.emitter.lifetime, Lifetime::Expression { .. })
        {
            self.done = true;
        }
    }

    fn eval3(&mut self, vars: &mut Vec<f32>, programs: &[Program; 3]) -> [f32; 3] {
        std::array::from_fn(|i| programs[i].eval(vars, &mut self.rng, &self.queries))
    }

    fn spawn_particle(&mut self, output: &mut Outputs) {
        let def = Arc::clone(&self.def);
        let mut vars = self.vars.clone();
        for index in 0..4u16 {
            let value = self.rng.unit();
            set_var(&mut vars, V_PARTICLE_RANDOM + index, value);
        }
        set_var(&mut vars, V_PARTICLE_AGE, 0.0);
        let lifetime = def
            .particle
            .max_lifetime
            .eval(&mut vars, &mut self.rng, &self.queries)
            .max(1e-3);
        set_var(&mut vars, V_PARTICLE_LIFETIME, lifetime);

        let (offset, direction) = self.sample_shape(&mut vars);
        let offset = if def.emitter.local_position {
            offset
        } else {
            transform(&self.basis, offset)
        };
        // Local-space velocity stays in the emitter frame and is rotated as the particle moves.
        let local_velocity = def.emitter.local_velocity;
        let direction = if local_velocity {
            direction
        } else {
            transform(&self.basis, direction)
        };
        let mut velocity = if let Some(velocity) = &def.particle.initial_velocity {
            let vector = self.eval3(&mut vars, velocity);
            if local_velocity {
                vector
            } else {
                transform(&self.basis, vector)
            }
        } else {
            let speed = match &def.particle.initial_speed {
                Some(speed) => speed.eval(&mut vars, &mut self.rng, &self.queries),
                None => 0.0,
            };
            direction.map(|c| c * speed)
        };
        for (component, extra) in velocity.iter_mut().zip(self.inherit_velocity) {
            *component += extra;
        }
        let (rotation, rotation_rate) = match &def.particle.spin {
            Some(spin) => (
                spin.rotation.eval(&mut vars, &mut self.rng, &self.queries),
                spin.rate.eval(&mut vars, &mut self.rng, &self.queries),
            ),
            None => (0.0, 0.0),
        };
        let pos = if def.emitter.local_position {
            offset
        } else {
            std::array::from_fn(|i| self.pos[i] + offset[i])
        };
        let mut particle = Particle::new(pos, velocity, lifetime, vars);
        particle.rotation = rotation;
        particle.rotation_rate = rotation_rate;
        if let Some(plane) = def.particle.kill_plane {
            let rel: [f32; 3] = if def.emitter.local_position {
                pos
            } else {
                std::array::from_fn(|i| pos[i] - self.pos[i])
            };
            particle.plane_side =
                plane[0] * rel[0] + plane[1] * rel[1] + plane[2] * rel[2] + plane[3];
        }
        let events = def.particle.creation_events.clone();
        self.particles.push_back(particle);
        let world_pos = self.world_position(self.particles.len() - 1);
        for event in events {
            self.run_event(&event, world_pos, velocity, output);
        }
    }

    pub fn world_position(&self, index: usize) -> [f32; 3] {
        let p = self.particles[index].pos;
        if self.def.emitter.local_position {
            let rotated = transform(&self.basis, p);
            std::array::from_fn(|i| self.pos[i] + rotated[i])
        } else {
            p
        }
    }

    fn sample_shape(&mut self, vars: &mut Vec<f32>) -> ([f32; 3], [f32; 3]) {
        let def = Arc::clone(&self.def);
        let shape = &def.emitter.shape;
        let base = self.eval3(vars, &shape.common.offset);
        let (point, outwards): ([f32; 3], Option<[f32; 3]>) = match &shape.kind {
            Shape::Point | Shape::Custom => ([0.0; 3], None),
            Shape::Sphere { radius } => {
                let radius = radius.eval(vars, &mut self.rng, &self.queries);
                let direction = self.random_unit();
                let distance = if shape.common.surface_only {
                    radius
                } else {
                    radius * self.rng.unit().cbrt()
                };
                (direction.map(|c| c * distance), Some(direction))
            }
            Shape::Box { half } => {
                let half = self.eval3(vars, half);
                (self.sample_box(half, shape.common.surface_only), None)
            }
            Shape::EntityAabb => {
                let dimension = |vars: &Vec<f32>, name: &str, default: f32| {
                    def.interner
                        .get(name)
                        .and_then(|slot| vars.get(slot as usize).copied())
                        .filter(|value| !value.is_nan())
                        .unwrap_or(default)
                };
                let half = [
                    dimension(vars, "aabb.x", 0.6) * 0.5,
                    dimension(vars, "aabb.y", 1.8) * 0.5,
                    dimension(vars, "aabb.z", 0.6) * 0.5,
                ];
                let mut p = self.sample_box(half, shape.common.surface_only);
                p[1] += half[1];
                (p, None)
            }
            Shape::Disc { radius, normal } => {
                let radius = radius.eval(vars, &mut self.rng, &self.queries);
                let normal = self.eval3(vars, normal);
                let normal = normalized(normal).unwrap_or([0.0, 1.0, 0.0]);
                let helper = if normal[1].abs() < 0.9 {
                    [0.0, 1.0, 0.0]
                } else {
                    [1.0, 0.0, 0.0]
                };
                let u = normalized(cross(helper, normal)).unwrap_or([1.0, 0.0, 0.0]);
                let v = cross(normal, u);
                let angle = self.rng.unit() * std::f32::consts::TAU;
                let distance = if shape.common.surface_only {
                    radius
                } else {
                    radius * self.rng.unit().sqrt()
                };
                let p: [f32; 3] =
                    std::array::from_fn(|i| (u[i] * angle.cos() + v[i] * angle.sin()) * distance);
                (p, normalized(p))
            }
        };
        let position: [f32; 3] = std::array::from_fn(|i| base[i] + point[i]);
        let outwards = outwards
            .or_else(|| normalized(point))
            .unwrap_or_else(|| self.random_unit());
        let direction = match &shape.common.direction {
            Direction::Outwards => outwards,
            Direction::Inwards => outwards.map(|c| -c),
            Direction::Vector(vector) => normalized(self.eval3(vars, vector)).unwrap_or([0.0; 3]),
        };
        (position, direction)
    }

    fn random_unit(&mut self) -> [f32; 3] {
        let z = self.rng.range(-1.0, 1.0);
        let angle = self.rng.unit() * std::f32::consts::TAU;
        let ring = (1.0 - z * z).max(0.0).sqrt();
        [ring * angle.cos(), z, ring * angle.sin()]
    }

    fn sample_box(&mut self, half: [f32; 3], surface_only: bool) -> [f32; 3] {
        let mut p = [
            self.rng.range(-half[0], half[0]),
            self.rng.range(-half[1], half[1]),
            self.rng.range(-half[2], half[2]),
        ];
        if surface_only {
            let axis = (self.rng.next_u32() % 3) as usize;
            p[axis] = if self.rng.next_u32() & 1 == 0 {
                -half[axis]
            } else {
                half[axis]
            };
        }
        p
    }

    /// Runs a named event of this effect at a world position.
    pub fn run_event(
        &mut self,
        name: &str,
        position: [f32; 3],
        velocity: [f32; 3],
        output: &mut Outputs,
    ) {
        let def = Arc::clone(&self.def);
        if let Some(node) = def.event(name) {
            self.run_node(node, position, velocity, output);
        }
    }

    fn run_node(
        &mut self,
        node: &EventNode,
        position: [f32; 3],
        velocity: [f32; 3],
        output: &mut Outputs,
    ) {
        match node {
            EventNode::Spawn { effect, kind } => {
                if self.depth >= MAX_SPAWN_DEPTH {
                    return;
                }
                let bound = match kind {
                    SpawnKind::EmitterBound => self.bound.map(|(actor, offset)| {
                        (
                            actor,
                            std::array::from_fn(|i| offset[i] + position[i] - self.pos[i]),
                        )
                    }),
                    _ => None,
                };
                output.spawns.push(SpawnRequest {
                    effect: effect.to_string(),
                    position,
                    bound,
                    basis: Some(self.basis),
                    queries: self.queries,
                    inherit_velocity: matches!(kind, SpawnKind::ParticleWithVelocity)
                        .then_some(velocity),
                    depth: self.depth + 1,
                    ..SpawnRequest::default()
                });
            }
            EventNode::Sound(name) => output.sounds.push(ParticleSound {
                name: name.clone(),
                position,
            }),
            EventNode::Expression(program) => {
                self.eval(program);
            }
            EventNode::Sequence(nodes) => {
                for node in nodes {
                    self.run_node(node, position, velocity, output);
                }
            }
            EventNode::Randomize(options) => {
                let total: f32 = options.iter().map(|(weight, _)| weight).sum();
                if total <= 0.0 {
                    return;
                }
                let mut pick = self.rng.unit() * total;
                for (weight, node) in options {
                    if pick < *weight {
                        self.run_node(node, position, velocity, output);
                        return;
                    }
                    pick -= weight;
                }
            }
        }
    }
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{atlas::ParticleAtlas, def::parse_effect};

    pub(crate) fn effect(json: &str) -> Arc<EffectDef> {
        Arc::new(parse_effect(json.as_bytes()).expect("test effect parses"))
    }

    fn emitter(json: &str, seed: u64) -> Emitter {
        let atlas = ParticleAtlas::default();
        Emitter::new(
            1,
            effect(json),
            atlas.fallback(),
            &SpawnRequest::default(),
            seed,
        )
    }

    const INSTANT: &str = r#"{"particle_effect":{"description":{"identifier":"t","basic_render_parameters":{"material":"particles_alpha","texture":"x"}},
      "components":{
        "minecraft:emitter_lifetime_once":{"active_time":1},
        "minecraft:emitter_rate_instant":{"num_particles":5},
        "minecraft:emitter_shape_sphere":{"radius":1,"direction":"outwards"},
        "minecraft:particle_initial_speed":2,
        "minecraft:particle_lifetime_expression":{"max_lifetime":1},
        "minecraft:particle_appearance_billboard":{"size":[0.1,0.1],"facing_camera_mode":"lookat_xyz","uv":{"uv":[0,0],"uv_size":[1,1]}}}}}"#;

    #[test]
    fn instant_rate_bursts_once_at_activation() {
        let mut e = emitter(INSTANT, 3);
        let mut out = Outputs::default();
        e.advance(0.05, &mut out, 1000);
        assert_eq!(e.particles.len(), 5);
        e.advance(0.05, &mut out, 1000);
        assert_eq!(e.particles.len(), 5);
    }

    #[test]
    fn outwards_velocity_has_the_initial_speed_magnitude() {
        let mut e = emitter(INSTANT, 9);
        e.advance(0.01, &mut Outputs::default(), 1000);
        for p in &e.particles {
            let speed = p.vel.iter().map(|c| c * c).sum::<f32>().sqrt();
            assert!((speed - 2.0).abs() < 1e-3, "{speed}");
        }
    }

    #[test]
    fn emitter_finishes_after_its_active_time() {
        let mut e = emitter(INSTANT, 3);
        e.advance(1.5, &mut Outputs::default(), 1000);
        assert!(e.done);
    }

    #[test]
    fn spawning_is_deterministic_per_seed() {
        let positions = |seed| {
            let mut e = emitter(INSTANT, seed);
            e.advance(0.01, &mut Outputs::default(), 1000);
            e.particles.iter().map(|p| p.pos).collect::<Vec<_>>()
        };
        assert_eq!(positions(5), positions(5));
        assert_ne!(positions(5), positions(6));
    }
}
