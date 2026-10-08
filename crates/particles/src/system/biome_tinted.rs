//! Native effect/colour cached manual emission for falling foliage.

use std::sync::Arc;

use super::{MAX_LIVE_PARTICLES, MAX_QUEUED_SOUNDS, ParticleSystem};
use crate::{
    def::{Motion, Rate},
    emitter::{Outputs, SpawnRequest},
};

impl ParticleSystem {
    /// Emits one biome-tinted particle at a block center. Vanilla 26.50 caches
    /// the emitter by effect name and colour compared as RGBA8, then requests
    /// one particle at each origin. The pack, not this route, controls its motion.
    pub fn spawn_biome_tinted(
        &mut self,
        effect: &str,
        block: [i32; 3],
        color: [f32; 4],
    ) -> Option<u64> {
        if color.iter().any(|component| !component.is_finite()) {
            self.dropped_spawns += 1;
            return None;
        }
        // Final manual emission adds .5 to each BlockPos component before the pack's shape.
        let position = block.map(|component| component as f32 + 0.5);
        let color = color.map(|component| component.clamp(0.0, 1.0));
        let key = color.map(|component| (component * 255.0) as u8);
        let variables = ["r", "g", "b", "a"]
            .into_iter()
            .zip(color)
            .map(|(component, value)| (format!("color.{component}"), value))
            .collect();
        let request = SpawnRequest {
            effect: effect.to_owned(),
            position,
            variables,
            manual_count: Some(0),
            ..SpawnRequest::default()
        };
        let Some(def) = self.resolve(effect).cloned() else {
            return self.spawn(&request);
        };
        // Existing parametric/kill-plane evaluation is emitter-relative even
        // for world-space particles. Keep unsupported custom overrides separate
        // so later block origins cannot move or retest earlier particles.
        if !matches!(def.emitter.rate, Rate::Manual { .. })
            || def.emitter.local_position
            || matches!(def.particle.motion, Motion::Parametric { .. })
            || def.particle.kill_plane.is_some()
        {
            return self.spawn(&SpawnRequest {
                manual_count: Some(1),
                ..request
            });
        }
        let distance_sq: f32 = (0..3)
            .map(|axis| (position[axis] - self.camera[axis]).powi(2))
            .sum();
        if distance_sq > super::MAX_SPAWN_DISTANCE.powi(2) {
            self.dropped_spawns += 1;
            return None;
        }
        let id = self
            .emitters
            .iter()
            .find(|emitter| {
                !emitter.done
                    && emitter.biome_tinted_key == Some(key)
                    && Arc::ptr_eq(&emitter.def, &def)
            })
            .map(|emitter| emitter.id)
            .or_else(|| self.spawn(&request))?;
        let live_budget = MAX_LIVE_PARTICLES.saturating_sub(self.live_particles());
        let emitter = self.emitters.iter_mut().find(|emitter| emitter.id == id)?;
        emitter.biome_tinted_key = Some(key);
        let mut output = Outputs::default();
        emitter.emit_cached_manual(position, &mut output, live_budget);
        self.sounds.append(&mut output.sounds);
        let excess = self.sounds.len().saturating_sub(MAX_QUEUED_SOUNDS);
        self.sounds.drain(..excess);
        for request in output.spawns {
            self.spawn(&request);
        }
        Some(id)
    }
}

#[cfg(test)]
mod tests;
