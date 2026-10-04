//! The live particle world: emitter registry, spawn API, fixed-budget ticking.

use std::sync::Arc;

use assets::RuntimeParticleAssets;
use bevy::prelude::Resource;

use super::{
    atlas::{ParticleAtlas, Placement},
    def::TextureSource,
    emitter::{Emitter, Outputs, ParticleSound, SpawnRequest},
    library::EffectLibrary,
    world::ParticleWorld,
};

mod biome_tinted;

/// Live particles across all emitters; the oldest are dropped past this.
pub const MAX_LIVE_PARTICLES: usize = 8192;
pub const MAX_EMITTERS: usize = 768;
/// Spawn requests farther than this from the camera are dropped.
pub const MAX_SPAWN_DISTANCE: f32 = 128.0;
/// Longest single simulation step; larger frame gaps are split.
const MAX_STEP: f32 = 1.0 / 30.0;
const MAX_FRAME_SECONDS: f32 = 0.25;
/// `_addTerrainEffect` checks the selected effect's
/// existing emitter/particle totals, with strict `>` comparisons before spawn.
const TERRAIN_EMITTER_LIMIT: usize = 20;
const TERRAIN_PARTICLE_LIMIT: usize = 500;
const TILE_VARIABLE_NAMES: [(&str, &str); 5] = [
    ("emitter_texture_coordinate", "emitter_texture_size"),
    ("emittertexturecoord", "emittertexturesize"),
    (
        "dig_particle_texture_coordinate",
        "dig_particle_texture_size",
    ),
    (
        "ground_particle_texture_coordinate",
        "ground_particle_texture_size",
    ),
    (
        "surface_particle_texture_coordinate",
        "surface_particle_texture_size",
    ),
];
/// Undrained sound requests kept before the oldest are dropped.
const MAX_QUEUED_SOUNDS: usize = 256;

#[derive(Resource, Default)]
pub struct ParticleSystem {
    library: EffectLibrary,
    atlas: ParticleAtlas,
    emitters: Vec<Emitter>,
    sounds: Vec<ParticleSound>,
    next_id: u64,
    seed: u64,
    camera: [f32; 3],
    boxes: Vec<[f32; 6]>,
    /// Requests dropped for an unknown effect, distance or a full emitter table.
    pub dropped_spawns: u64,
}

impl ParticleSystem {
    /// Builds the system from the compiled carrier; effects parse eagerly.
    #[must_use]
    pub fn from_assets(assets: &RuntimeParticleAssets) -> Self {
        Self {
            library: EffectLibrary::from_assets(assets),
            atlas: ParticleAtlas::from_assets(assets),
            ..Self::default()
        }
    }

    /// Registers or overrides an effect from raw json (joined server packs).
    pub fn register_effect(&mut self, bytes: &[u8]) -> bool {
        self.library.insert(bytes)
    }

    #[must_use]
    pub fn has_effect(&self, name: &str) -> bool {
        self.resolve(name).is_some()
    }

    #[must_use]
    pub fn effect_count(&self) -> usize {
        self.library.len()
    }

    #[must_use]
    pub fn atlas(&self) -> &ParticleAtlas {
        &self.atlas
    }

    pub fn set_camera(&mut self, position: [f32; 3]) {
        self.camera = position;
    }

    #[must_use]
    pub fn camera(&self) -> [f32; 3] {
        self.camera
    }

    #[must_use]
    pub fn live_particles(&self) -> usize {
        self.emitters.iter().map(|e| e.particles.len()).sum()
    }

    #[must_use]
    pub fn emitter_count(&self) -> usize {
        self.emitters.len()
    }

    /// Removes every emitter and particle (dimension change, disconnect).
    pub fn clear(&mut self) {
        self.emitters.clear();
        self.sounds.clear();
    }

    pub(super) fn atlas_base(&mut self) -> std::sync::Arc<[u8]> {
        self.atlas.base()
    }

    pub(super) fn emitters_mut(&mut self) -> &mut [Emitter] {
        &mut self.emitters
    }

    fn resolve(&self, name: &str) -> Option<&Arc<super::def::EffectDef>> {
        self.library.get(name).or_else(|| {
            if name.contains(':') {
                None
            } else {
                self.library.get(&format!("minecraft:{name}"))
            }
        })
    }

    /// Starts an emitter; returns its id, or `None` when the effect is unknown, too far away,
    /// or the emitter table is full.
    pub fn spawn(&mut self, request: &SpawnRequest) -> Option<u64> {
        let Some(def) = self.resolve(&request.effect).cloned() else {
            self.dropped_spawns += 1;
            return None;
        };
        let distance_sq: f32 = (0..3)
            .map(|i| (request.position[i] - self.camera[i]).powi(2))
            .sum();
        if !distance_sq.is_finite() || distance_sq > MAX_SPAWN_DISTANCE * MAX_SPAWN_DISTANCE {
            self.dropped_spawns += 1;
            return None;
        }
        if self.emitters.len() >= MAX_EMITTERS {
            self.emitters.remove(0);
        }
        let mut request = request.clone();
        let texture = self.resolve_texture(&def.texture, &mut request);
        self.next_id += 1;
        let id = self.next_id;
        let seed = request.seed
            ^ self.seed.wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ id.wrapping_mul(0xD6E8_FEB8_6659_FD93)
            ^ u64::from(request.position[0].to_bits())
            ^ (u64::from(request.position[2].to_bits()) << 32);
        self.emitters
            .push(Emitter::new(id, def, texture, &request, seed));
        Some(id)
    }

    /// Native terrain-effect admission, independent of unrelated particle effects.
    /// The threshold is not a clamp on this burst: an admitted burst may cross it.
    pub fn spawn_terrain(&mut self, request: &SpawnRequest) -> Option<u64> {
        let Some(def) = self.resolve(&request.effect) else {
            return self.spawn(request);
        };
        let mut emitters = 0;
        let mut particles = 0;
        for emitter in &self.emitters {
            if emitter.def.identifier == def.identifier {
                emitters += 1;
                particles += emitter.particles.len();
            }
        }
        if emitters > TERRAIN_EMITTER_LIMIT || particles > TERRAIN_PARTICLE_LIMIT {
            self.dropped_spawns += 1;
            return None;
        }
        self.spawn(request)
    }

    fn resolve_texture(&mut self, source: &TextureSource, request: &mut SpawnRequest) -> Placement {
        self.atlas.set_live_placements(
            self.emitters
                .iter()
                .filter(|emitter| !emitter.done || !emitter.particles.is_empty())
                .map(|emitter| emitter.texture),
        );
        let fallback = self.atlas.fallback();
        match source {
            TextureSource::Path(path) => self.atlas.placement(path).unwrap_or(fallback),
            TextureSource::Terrain | TextureSource::Items => {
                let Some(tile) = request.tile.as_ref() else {
                    return fallback;
                };
                let placement = self
                    .atlas
                    .tile(tile.key, tile.size, &tile.pixels)
                    .unwrap_or(fallback);
                let [u, v, du, dv] = placement.normalized();
                // Vanilla effects name the same tile rectangle several ways.
                for (coordinate, size) in TILE_VARIABLE_NAMES {
                    for (name, value) in [
                        (format!("{coordinate}.u"), u),
                        (format!("{coordinate}.v"), v),
                        (format!("{size}.u"), du),
                        (format!("{size}.v"), dv),
                    ] {
                        request.variables.push((name, value));
                    }
                }
                placement
            }
        }
    }

    /// `(emitter id, actor runtime id, offset)` for every actor-bound emitter; the host sets each
    /// emitter's transform from the actor pose, or stops it when the actor is gone.
    #[must_use]
    pub fn bound_emitters(&self) -> Vec<(u64, u64, [f32; 3])> {
        self.emitters
            .iter()
            .filter(|e| !e.done)
            .filter_map(|e| e.bound.map(|(actor, offset)| (e.id, actor, offset)))
            .collect()
    }

    /// Updates each active actor-bound emitter once. A missing transform stops its emission;
    /// its existing particles finish as usual.
    pub fn update_bound_emitters(
        &mut self,
        mut transform: impl FnMut(u64, [f32; 3]) -> Option<([f32; 3], [[f32; 3]; 3])>,
    ) {
        for emitter in &mut self.emitters {
            if emitter.done {
                continue;
            }
            let Some((actor, offset)) = emitter.bound else {
                continue;
            };
            if let Some((position, basis)) = transform(actor, offset) {
                emitter.pos = position;
                emitter.basis = basis;
            } else {
                emitter.done = true;
            }
        }
    }

    /// Moves an attached emitter; ignored once the emitter is gone.
    pub fn set_transform(&mut self, id: u64, position: [f32; 3], basis: [[f32; 3]; 3]) {
        if let Some(emitter) = self.emitters.iter_mut().find(|e| e.id == id) {
            emitter.pos = position;
            emitter.basis = basis;
        }
    }

    /// Stops an emitter from emitting; its live particles finish out.
    pub fn stop(&mut self, id: u64) {
        if let Some(emitter) = self.emitters.iter_mut().find(|e| e.id == id) {
            emitter.done = true;
        }
    }

    /// Sound requests (`sound_effect` events) raised since the last call; the audio runtime
    /// drains this and resolves `event_name` against the sound catalog.
    pub fn take_sounds(&mut self) -> Vec<ParticleSound> {
        std::mem::take(&mut self.sounds)
    }

    /// Advances every emitter by `dt` seconds against `world`.
    pub fn tick(&mut self, dt: f32, world: &dyn ParticleWorld) {
        if !dt.is_finite() || dt <= 0.0 {
            return;
        }
        let mut remaining = dt.min(MAX_FRAME_SECONDS);
        while remaining > 1e-6 {
            let step = remaining.min(MAX_STEP);
            remaining -= step;
            self.step(step, world);
        }
        self.enforce_budget();
    }

    fn step(&mut self, dt: f32, world: &dyn ParticleWorld) {
        let mut output = Outputs::default();
        let live = self.live_particles();
        let mut budget = MAX_LIVE_PARTICLES.saturating_sub(live);
        let mut boxes = std::mem::take(&mut self.boxes);
        for emitter in &mut self.emitters {
            let before = emitter.particles.len();
            emitter.advance(dt, &mut output, budget);
            budget = budget.saturating_sub(emitter.particles.len().saturating_sub(before));
            emitter.update_particles(dt, world, &mut boxes, &mut output);
        }
        self.boxes = boxes;
        self.emitters.retain(|emitter| !emitter.is_finished());
        self.sounds.append(&mut output.sounds);
        if self.sounds.len() > MAX_QUEUED_SOUNDS {
            let excess = self.sounds.len() - MAX_QUEUED_SOUNDS;
            self.sounds.drain(..excess);
        }
        self.seed = self.seed.wrapping_add(1);
        for request in output.spawns {
            self.spawn(&request);
        }
    }

    /// Drops the oldest particles until the live count fits the budget.
    fn enforce_budget(&mut self) {
        let mut excess = self.live_particles().saturating_sub(MAX_LIVE_PARTICLES);
        while excess > 0 {
            let Some(victim) = self
                .emitters
                .iter_mut()
                .filter(|e| !e.particles.is_empty())
                .min_by_key(|e| e.id)
            else {
                return;
            };
            let take = excess.min(victim.particles.len());
            victim.particles.drain(..take);
            excess -= take;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::particles::world::EmptyWorld;

    const EFFECT: &str = r#"{"particle_effect":{"description":{"identifier":"minecraft:burst","basic_render_parameters":{"material":"particles_alpha","texture":"x"}},
      "components":{
        "minecraft:emitter_lifetime_once":{"active_time":1},
        "minecraft:emitter_rate_instant":{"num_particles":4},
        "minecraft:emitter_shape_point":{},
        "minecraft:particle_lifetime_expression":{"max_lifetime":0.5},
        "minecraft:particle_appearance_billboard":{"size":[0.1,0.1],"facing_camera_mode":"lookat_xyz","uv":{"uv":[0,0],"uv_size":[1,1]}}}}}"#;

    fn system() -> ParticleSystem {
        let mut system = ParticleSystem::default();
        assert!(system.register_effect(EFFECT.as_bytes()));
        system
    }

    fn request(name: &str, x: f32) -> SpawnRequest {
        SpawnRequest {
            effect: name.into(),
            position: [x, 0.0, 0.0],
            ..SpawnRequest::default()
        }
    }

    #[test]
    fn spawn_resolves_bare_names_and_bursts_particles() {
        let mut system = system();
        assert!(system.spawn(&request("burst", 0.0)).is_some());
        system.tick(0.05, &EmptyWorld);
        assert_eq!(system.live_particles(), 4);
    }

    #[test]
    fn unknown_and_distant_effects_are_dropped_and_counted() {
        let mut system = system();
        assert!(system.spawn(&request("missing", 0.0)).is_none());
        assert!(system.spawn(&request("burst", 500.0)).is_none());
        assert_eq!(system.dropped_spawns, 2);
    }

    #[test]
    fn finished_emitters_are_removed_once_particles_die() {
        let mut system = system();
        system.spawn(&request("burst", 0.0));
        system.tick(0.1, &EmptyWorld);
        assert_eq!(system.emitter_count(), 1);
        // One tick advances at most a quarter second.
        for _ in 0..4 {
            system.tick(0.25, &EmptyWorld);
        }
        assert_eq!(system.emitter_count(), 0);
    }

    #[test]
    fn oldest_particles_are_dropped_past_the_budget() {
        let mut system = ParticleSystem::default();
        let heavy = EFFECT
            .replace("minecraft:burst", "minecraft:heavy")
            .replace("\"num_particles\":4", "\"num_particles\":100");
        assert!(system.register_effect(heavy.as_bytes()));
        for _ in 0..(MAX_LIVE_PARTICLES / 100 + 20) {
            system.spawn(&request("heavy", 0.0));
        }
        system.tick(0.02, &EmptyWorld);
        assert!(system.live_particles() <= MAX_LIVE_PARTICLES);
    }

    #[test]
    fn terrain_admission_uses_strict_per_effect_emitter_threshold() {
        let mut system = system();
        let req = request("burst", 0.0);
        for _ in 0..=TERRAIN_EMITTER_LIMIT {
            assert!(system.spawn_terrain(&req).is_some());
        }
        assert!(system.spawn_terrain(&req).is_none());
        assert_eq!(system.emitter_count(), TERRAIN_EMITTER_LIMIT + 1);
        let other = EFFECT.replace("minecraft:burst", "minecraft:other");
        assert!(system.register_effect(other.as_bytes()));
        assert!(system.spawn_terrain(&request("other", 0.0)).is_some());
    }

    #[test]
    fn terrain_admission_does_not_clamp_an_admitted_burst() {
        let mut system = system();
        let large = EFFECT.replace(
            "\"num_particles\":4",
            &format!("\"num_particles\":{TERRAIN_PARTICLE_LIMIT}"),
        );
        assert!(system.register_effect(large.as_bytes()));
        let req = request("burst", 0.0);
        assert!(system.spawn_terrain(&req).is_some());
        system.tick(0.01, &EmptyWorld);
        assert_eq!(system.live_particles(), TERRAIN_PARTICLE_LIMIT);
        assert!(system.spawn_terrain(&req).is_some());
        system.tick(0.01, &EmptyWorld);
        assert!(system.live_particles() > TERRAIN_PARTICLE_LIMIT);
        assert!(system.spawn_terrain(&req).is_none());
    }

    #[test]
    fn replacing_a_pack_effect_does_not_reset_terrain_admission() {
        let mut system = system();
        let req = request("burst", 0.0);
        for _ in 0..=TERRAIN_EMITTER_LIMIT {
            assert!(system.spawn_terrain(&req).is_some());
        }
        assert!(system.register_effect(EFFECT.as_bytes()));
        assert!(system.spawn_terrain(&req).is_none());
    }

    /// Starts bound and unbound emitters with identical seeded particles for comparisons.
    fn bound_system(count: u64) -> ParticleSystem {
        let mut system = system();
        for actor in 0..count {
            system.spawn(&SpawnRequest {
                bound: Some((actor, [1.0, 2.0, 3.0])),
                ..request("burst", 0.0)
            });
        }
        system.spawn(&request("burst", 0.0));
        system.tick(0.02, &EmptyWorld);
        system
    }

    /// Reproduces the host's previous allocate-and-find refresh for exact comparisons.
    fn old_bound_refresh(
        system: &mut ParticleSystem,
        mut transform: impl FnMut(u64, [f32; 3]) -> Option<([f32; 3], [[f32; 3]; 3])>,
    ) {
        for (id, actor, offset) in system.bound_emitters() {
            match transform(actor, offset) {
                Some((position, basis)) => system.set_transform(id, position, basis),
                None => system.stop(id),
            }
        }
    }

    #[test]
    fn bound_refresh_visits_each_active_attachment_once_and_matches_the_old_path() {
        let mut old = bound_system(4);
        let mut new = bound_system(4);
        // A stopped emitter can still contain particles; neither refresh visits it again.
        old.stop(1);
        new.stop(1);
        let mut visited = Vec::new();
        let transform = |actor, offset: [f32; 3]| {
            (actor != 2).then_some((
                [offset[0] + actor as f32, offset[1], offset[2]],
                [[0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
            ))
        };
        old_bound_refresh(&mut old, transform);
        new.update_bound_emitters(|actor, offset| {
            visited.push(actor);
            transform(actor, offset)
        });
        assert_eq!(visited, [1, 2, 3]);
        for (old, new) in old.emitters.iter().zip(&new.emitters) {
            assert_eq!(old.pos, new.pos);
            assert_eq!(old.basis, new.basis);
            assert_eq!(old.done, new.done);
            assert_eq!(old.particles.len(), new.particles.len());
        }
        assert!(new.emitters[2].done, "missing actor stops new emission");
        assert_eq!(new.emitters[2].particles.len(), 4, "live particles remain");
        let view = super::super::draw::ParticleView {
            position: [0.0, 0.0, 10.0],
            right: [1.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            forward: [0.0, 0.0, -1.0],
            half_diagonal: 1.0,
        };
        for _ in 0..4 {
            old.tick(0.02, &EmptyWorld);
            new.tick(0.02, &EmptyWorld);
            let old_draw = old.build_draw(&view, &EmptyWorld);
            let new_draw = new.build_draw(&view, &EmptyWorld);
            assert_eq!(old_draw.blend, new_draw.blend);
            assert_eq!(old_draw.add, new_draw.add);
        }
        visited.clear();
        new.update_bound_emitters(|actor, offset| {
            visited.push(actor);
            transform(actor, offset)
        });
        assert_eq!(visited, [1, 3]);
    }

    #[test]
    #[ignore = "benchmark"]
    fn frame_cost_bench_bound_particle_emitters_768() {
        let transform = |actor, offset: [f32; 3]| {
            Some((
                [offset[0] + actor as f32, offset[1], offset[2]],
                [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            ))
        };
        let frames = 500;
        // The cap is 768; leave out the unbound comparison emitter so every slot is attached.
        let mut old = bound_system(MAX_EMITTERS as u64 - 1);
        old.emitters.pop();
        old.spawn(&SpawnRequest {
            bound: Some((MAX_EMITTERS as u64 - 1, [1.0, 2.0, 3.0])),
            ..request("burst", 0.0)
        });
        let started = std::time::Instant::now();
        for _ in 0..frames {
            old_bound_refresh(&mut old, std::hint::black_box(transform));
        }
        let old_time = started.elapsed() / frames;
        let mut new = bound_system(MAX_EMITTERS as u64 - 1);
        new.emitters.pop();
        new.spawn(&SpawnRequest {
            bound: Some((MAX_EMITTERS as u64 - 1, [1.0, 2.0, 3.0])),
            ..request("burst", 0.0)
        });
        let started = std::time::Instant::now();
        for _ in 0..frames {
            new.update_bound_emitters(std::hint::black_box(transform));
        }
        let new_time = started.elapsed() / frames;
        assert_eq!(new.bound_emitters().len(), MAX_EMITTERS);
        eprintln!(
            "FRAME_COST bound_particle_emitters_768: old={:.3}ms new={:.3}ms",
            old_time.as_secs_f64() * 1e3,
            new_time.as_secs_f64() * 1e3,
        );
    }
    #[test]
    fn review_render_local_spawn_offset_is_transformed_once() {
        let effect = EFFECT.replace("minecraft:emitter_shape_point\":{}", "minecraft:emitter_shape_point\":{\"offset\":[1,0,0]}")
            .replace("\"minecraft:emitter_lifetime_once\"", "\"minecraft:emitter_local_space\":{\"position\":true},\"minecraft:emitter_lifetime_once\"");
        let mut system = ParticleSystem::default();
        assert!(system.register_effect(effect.as_bytes()));
        let mut request = request("burst", 0.0);
        request.basis = Some([[0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]]);
        system.spawn(&request).unwrap();
        system.tick(0.01, &EmptyWorld);
        assert_eq!(system.emitters[0].world_position(0), [0.0, 0.0, -1.0]);
    }

    #[test]
    fn review_render_stopped_manual_emitter_does_not_burst() {
        let mut system = ParticleSystem::default();
        let effect = EFFECT.replace(
            "minecraft:emitter_rate_instant",
            "minecraft:emitter_rate_manual",
        );
        assert!(system.register_effect(effect.as_bytes()));
        let mut request = request("burst", 0.0);
        request.manual_count = Some(3);
        let id = system.spawn(&request).unwrap();
        system.stop(id);
        system.tick(0.01, &EmptyWorld);
        assert_eq!(system.live_particles(), 0);
    }
}
