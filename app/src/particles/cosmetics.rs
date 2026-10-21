//! Catalog cosmetic emitters follow the attachments the actor frame draws in the world.
//! Player previews own a separate simulation in the UI attachment frame.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use client_presentation::actor_publication::{CosmeticEmitterKey, CosmeticEmitterPlacement};
use particles::{ParticleSystem, SpawnRequest, TileRequest};

#[cfg(test)]
mod tests;

/// Players whose items may all emit at once when every item declares the per-item maximum.
const FULLY_EMITTING_PLAYERS: usize = 16;
/// Most cosmetic emitters in the particle table at once (128 with eight per item), at most a
/// quarter of it. Stopped emitters count until their particles drain; placements beyond the
/// budget wait, in submission order, for a free slot.
pub(super) const MAX_COSMETIC_EMITTERS: usize =
    FULLY_EMITTING_PLAYERS * render_model::hats::MAX_COSMETIC_EMITTERS;
const _: () = assert!(MAX_COSMETIC_EMITTERS <= particles::MAX_EMITTERS / 4);

/// One live emitter and the sync pass that last saw its placement.
struct Live {
    id: u64,
    seen: u64,
}

/// Emitters started for cosmetic placements in one particle system.
#[derive(Default)]
pub(super) struct CosmeticEmitters {
    instance: Option<u64>,
    live: HashMap<CosmeticEmitterKey, Live>,
    /// Stopped or finished emitters that still occupy the table while their particles drain.
    draining: Vec<u64>,
    /// Effects this system refused to parse; they are not offered again until it is rebuilt.
    rejected: HashSet<Arc<str>>,
    pass: u64,
}

impl CosmeticEmitters {
    /// Forgets every emitter after the host cleared the system or left the world.
    pub(super) fn reset(&mut self) {
        self.live.clear();
        self.draining.clear();
    }

    /// Emitters this tracker holds in the particle table, including draining ones.
    fn occupied(&self) -> usize {
        self.live.len() + self.draining.len()
    }

    /// Starts emitters for new placements, moves existing ones and stops those whose items are
    /// unequipped, hidden or despawned. A rebuilt system drops every held id; an emitter that
    /// finished or was evicted restarts while its placement remains.
    pub(super) fn sync(
        &mut self,
        system: &mut ParticleSystem,
        placements: &[CosmeticEmitterPlacement],
    ) {
        if self.instance != Some(system.instance()) {
            self.instance = Some(system.instance());
            self.live.clear();
            self.draining.clear();
            self.rejected.clear();
        }
        self.draining.retain(|id| system.contains_emitter(*id));
        self.pass = self.pass.wrapping_add(1);
        let pass = self.pass;
        for placement in placements {
            let Some(live) = self.live.get_mut(&placement.key) else {
                continue;
            };
            if system.is_emitting(live.id) {
                live.seen = pass;
                let transform = placement.transform;
                system.set_transform(live.id, transform.position, transform.basis);
            }
        }
        let draining = &mut self.draining;
        self.live.retain(|_, live| {
            if live.seen == pass {
                return true;
            }
            system.stop(live.id);
            if system.contains_emitter(live.id) {
                draining.push(live.id);
            }
            false
        });
        for placement in placements {
            if self.occupied() >= MAX_COSMETIC_EMITTERS
                || system.emitter_count() >= particles::MAX_EMITTERS
            {
                break;
            }
            if self.live.contains_key(&placement.key) || !self.admit_effect(system, placement) {
                continue;
            }
            let key = placement.key;
            let request = SpawnRequest {
                effect: placement.identifier.to_string(),
                position: placement.transform.position,
                basis: Some(placement.transform.basis),
                seed: key.runtime_id
                    ^ key.spawn_revision.rotate_left(32)
                    ^ (u64::from(key.item.index()) << 8 | u64::from(key.emitter)),
                tile: placement.sprite.as_ref().map(|(key, pixels)| TileRequest {
                    key: *key,
                    size: render_model::hats::COSMETIC_SPRITE_SIDE,
                    pixels: Arc::clone(pixels),
                }),
                ..SpawnRequest::default()
            };
            if let Some(id) = system.spawn(&request) {
                self.live.insert(key, Live { id, seen: pass });
            }
        }
    }

    /// Registers a placement's effect once per system, quietly skipping effects it rejects.
    fn admit_effect(
        &mut self,
        system: &mut ParticleSystem,
        placement: &CosmeticEmitterPlacement,
    ) -> bool {
        if system.has_effect(&placement.identifier) {
            return true;
        }
        if self.rejected.contains(&placement.identifier) {
            return false;
        }
        if system.register_effect(&placement.effect) && system.has_effect(&placement.identifier) {
            return true;
        }
        self.rejected.insert(Arc::clone(&placement.identifier));
        false
    }
}
