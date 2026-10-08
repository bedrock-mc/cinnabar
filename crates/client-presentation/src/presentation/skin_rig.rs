//! Player skins' own models registered as actor rig geometry, shared by model digest.
use assets::SkinGeometry;
use render_model::{ActorRigGeometry, EntityRigId, skin_rig_id};
use std::collections::{HashMap, HashSet, VecDeque};

/// Body and animated skin models for every selected player, plus the local first-person hand.
pub const MAX_SKIN_RIGS: usize =
    render_model::MAX_RENDERED_PLAYERS * (1 + protocol::MAX_SKIN_ANIMATION_LAYERS) + 1;
/// Models that failed to build, remembered so they are not rebuilt every frame.
const MAX_REJECTED_SKIN_RIGS: usize = 64;

struct Slot {
    digest: [u8; 32],
}

#[derive(Default)]
pub struct SkinRigCache {
    slots: Vec<Slot>,
    index: HashMap<[u8; 32], usize>,
    recency: render_model::FrameSlotRecency,
    rejected: HashSet<[u8; 32]>,
    rejection_order: VecDeque<[u8; 32]>,
    frame: u64,
}

impl SkinRigCache {
    /// Starts a selection pass without changing any model admission.
    pub fn begin_frame(&mut self) {
        self.frame = self.frame.wrapping_add(1);
    }

    /// Registers the model on first use; returns `None` when it cannot be built or no slot is free.
    pub fn rig(
        &mut self,
        geometry: &SkinGeometry,
        prepared: Option<&ActorRigGeometry>,
        mut register: impl FnMut(ActorRigGeometry),
    ) -> Option<EntityRigId> {
        #[cfg(test)]
        tests::record_probe();
        if let Some(&index) = self.index.get(&geometry.digest) {
            self.recency.touch(index, self.frame);
            return Some(skin_rig_id(index as u32));
        }
        if self.rejected.contains(&geometry.digest) {
            return None;
        }
        let index = if self.slots.len() < MAX_SKIN_RIGS {
            self.slots.len()
        } else {
            self.recency.oldest_unused(self.frame)?
        };
        let id = skin_rig_id(index as u32);
        let Some(mut built) = prepared.cloned() else {
            if self.rejected.len() == MAX_REJECTED_SKIN_RIGS
                && let Some(oldest) = self.rejection_order.pop_front()
            {
                self.rejected.remove(&oldest);
            }
            self.rejection_order.push_back(geometry.digest);
            self.rejected.insert(geometry.digest);
            bevy::log::warn!(identifier = %geometry.identifier, "skin model could not be built");
            return None;
        };
        built.id = id;
        register(built);
        let slot = Slot {
            digest: geometry.digest,
        };
        if index == self.slots.len() {
            self.slots.push(slot);
        } else {
            self.index.remove(&self.slots[index].digest);
            self.recency.remove(index);
            self.slots[index] = slot;
        }
        self.index.insert(geometry.digest, index);
        self.recency.touch(index, self.frame);
        Some(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PATCH: &str = r#"{"geometry":{"default":"geometry.test"}}"#;
    const MODEL: &str = r#"{"format_version":"1.12.0","minecraft:geometry":[{
 "description":{"identifier":"geometry.test","texture_width":64,"texture_height":64},
 "bones":[{"name":"body","pivot":[0,24,0],"cubes":[{"origin":[-4,12,-2],"size":[8,12,4],"uv":[16,16]}]}]}]}"#;

    /// More distinct models than slots in one frame must not evict models drawn this frame.
    #[test]
    fn a_crowded_frame_never_evicts_its_own_models() {
        let mut model = assets::parse_skin_geometry(PATCH, MODEL).unwrap().unwrap();
        let models: Vec<SkinGeometry> = (0..=MAX_SKIN_RIGS)
            .map(|index| {
                model.digest[..8].copy_from_slice(&(index as u64).to_le_bytes());
                model.clone()
            })
            .collect();
        let mut cache = SkinRigCache::default();
        let mut builds = 0;
        for frame in 0..3 {
            cache.begin_frame();
            let ids: Vec<_> = models
                .iter()
                .map(|model| {
                    cache.rig(
                        model,
                        render_model::skin_geometry(model, render_model::DIAGNOSTIC_RIG_ID)
                            .ok()
                            .as_ref(),
                        |_| builds += 1,
                    )
                })
                .collect();
            assert_eq!(ids.iter().flatten().count(), MAX_SKIN_RIGS, "frame {frame}");
            assert!(ids[MAX_SKIN_RIGS].is_none());
        }
        assert_eq!(builds, MAX_SKIN_RIGS, "each model built once");
    }

    #[test]
    fn every_selected_player_and_local_hand_can_have_a_distinct_skin_model() {
        let mut model = assets::parse_skin_geometry(PATCH, MODEL).unwrap().unwrap();
        let mut cache = SkinRigCache::default();
        cache.begin_frame();
        for index in 0..MAX_SKIN_RIGS {
            model.digest[..8].copy_from_slice(&(index as u64).to_le_bytes());
            assert!(
                cache
                    .rig(
                        &model,
                        render_model::skin_geometry(&model, render_model::DIAGNOSTIC_RIG_ID)
                            .ok()
                            .as_ref(),
                        |_| {}
                    )
                    .is_some(),
                "player {index}"
            );
        }
    }
    thread_local! {
        static LOOKUP_PROBES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    /// Counts one explicit candidate inspection without changing production code.
    pub(super) fn record_probe() {
        LOOKUP_PROBES.with(|count| count.set(count.get() + 1));
    }

    /// Starts an independent work sample on this test thread.
    fn reset_probes() {
        LOOKUP_PROBES.with(|count| count.set(0));
    }

    /// Returns deterministic lookup work rather than elapsed time.
    fn probes() -> usize {
        LOOKUP_PROBES.with(std::cell::Cell::get)
    }

    #[test]
    fn bounded_lookup_work_for_unique_and_unchanged_skin_models() {
        let mut model = assets::parse_skin_geometry(PATCH, MODEL).unwrap().unwrap();
        let models: Vec<_> = (0..render_model::MAX_RENDERED_PLAYERS)
            .map(|index| {
                model.digest[..8].copy_from_slice(&(index as u64).to_le_bytes());
                model.clone()
            })
            .collect();
        let mut cache = SkinRigCache::default();
        let mut builds = 0;
        reset_probes();
        for _ in 0..2 {
            cache.begin_frame();
            for model in &models {
                assert!(
                    cache
                        .rig(
                            model,
                            render_model::skin_geometry(model, render_model::DIAGNOSTIC_RIG_ID)
                                .ok()
                                .as_ref(),
                            |_| builds += 1
                        )
                        .is_some()
                );
            }
        }
        assert_eq!(builds, models.len());
        assert!(
            probes() <= models.len() * 4,
            "{} slot candidates for {} models",
            probes(),
            models.len()
        );
    }
}
