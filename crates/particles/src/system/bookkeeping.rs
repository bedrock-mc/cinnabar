//! Incremental totals for retained entries and the shorter lifetime of atlas owners.

use std::collections::HashMap;

use crate::{atlas::ParticleAtlas, emitter::Emitter};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct EffectCounts {
    pub(super) emitters: usize,
    pub(super) particles: usize,
}

#[derive(Default)]
pub(super) struct Bookkeeping {
    effects: HashMap<Box<str>, EffectCounts>,
    live_particles: usize,
}

impl Bookkeeping {
    /// Returns retained table entries and live particles, keyed by identifier across replacements.
    pub(super) fn effect(&self, identifier: &str) -> EffectCounts {
        self.effects.get(identifier).copied().unwrap_or_default()
    }

    /// Returns the total without walking the emitter table.
    pub(super) fn live_particles(&self) -> usize {
        self.live_particles
    }

    /// Admits one table entry and its current atlas ownership.
    pub(super) fn insert(&mut self, emitter: &Emitter, atlas: &mut ParticleAtlas) {
        let particles = emitter.particles.len();
        if let Some(counts) = self.effects.get_mut(emitter.def.identifier.as_ref()) {
            counts.emitters += 1;
            counts.particles += particles;
        } else {
            self.effects.insert(
                emitter.def.identifier.clone(),
                EffectCounts {
                    emitters: 1,
                    particles,
                },
            );
        }
        self.live_particles += particles;
        if protects_placement(emitter) {
            atlas.retain_placement(emitter.texture);
        }
    }

    /// Removes one table entry, releasing a pin only if it still owns one.
    pub(super) fn remove(&mut self, emitter: &Emitter, atlas: &mut ParticleAtlas) {
        let identifier = emitter.def.identifier.as_ref();
        let counts = self
            .effects
            .get_mut(identifier)
            .expect("emitter is counted");
        counts.emitters -= 1;
        counts.particles -= emitter.particles.len();
        self.live_particles -= emitter.particles.len();
        if counts.emitters == 0 {
            self.effects.remove(identifier);
        }
        if protects_placement(emitter) {
            atlas.release_placement(emitter.texture);
        }
    }

    /// Mutates one entry's lifetime or particles and applies its count and pin transition once.
    /// The operation must preserve the entry's definition and texture placement.
    pub(super) fn edit(
        &mut self,
        emitter: &mut Emitter,
        atlas: &mut ParticleAtlas,
        operation: impl FnOnce(&mut Emitter),
    ) {
        let before = emitter.particles.len();
        let protected = protects_placement(emitter);
        operation(emitter);
        let after = emitter.particles.len();
        if before != after {
            let counts = self
                .effects
                .get_mut(emitter.def.identifier.as_ref())
                .expect("emitter is counted");
            counts.particles = counts.particles - before + after;
            self.live_particles = self.live_particles - before + after;
        }
        match (protected, protects_placement(emitter)) {
            (true, false) => atlas.release_placement(emitter.texture),
            (false, true) => atlas.retain_placement(emitter.texture),
            _ => {}
        }
    }

    /// Clears the accounting and pins while leaving reusable atlas pixels intact.
    pub(super) fn clear(&mut self, atlas: &mut ParticleAtlas) {
        self.effects.clear();
        self.live_particles = 0;
        atlas.clear_placements();
    }
}

/// A stopped emitter keeps its pixels until its final particle leaves.
fn protects_placement(emitter: &Emitter) -> bool {
    !emitter.done || !emitter.particles.is_empty()
}

#[cfg(test)]
mod tests;
