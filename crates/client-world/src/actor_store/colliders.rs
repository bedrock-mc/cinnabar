//! Retained collision bounds owned by solid actor membership.

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use super::ActorSnapshot;

/// Feet-anchored minimum and maximum corners, before simulation coordinate conversion.
pub type ActorCollisionBounds = ([f32; 3], [f32; 3]);

#[derive(Debug, Clone, Copy)]
struct Collider {
    unique_id: i64,
    bounds: Option<ActorCollisionBounds>,
}

impl PartialEq for Collider {
    /// Compares exact coordinates, including retained non-finite values and signed zeroes.
    fn eq(&self, other: &Self) -> bool {
        let bits = |bounds: Option<ActorCollisionBounds>| {
            bounds.map(|(min, max)| (min.map(f32::to_bits), max.map(f32::to_bits)))
        };
        self.unique_id == other.unique_id && bits(self.bounds) == bits(other.bounds)
    }
}

#[derive(Debug)]
struct Snapshot {
    mover: u64,
    mount: Option<i64>,
    bounds: Arc<[ActorCollisionBounds]>,
}

/// Solid membership and bounds; only the latest filtered snapshot is retained here.
#[derive(Debug, Default)]
pub(super) struct ActorColliders {
    members: BTreeMap<u64, Collider>,
    snapshot: Mutex<Option<Snapshot>>,
    #[cfg(test)]
    snapshot_visits: std::sync::atomic::AtomicUsize,
}

impl ActorColliders {
    /// Refreshes one actor after its pose, collision metadata or constructor changes.
    pub(super) fn update(&mut self, actor: &ActorSnapshot) {
        let collider = actor.is_collidable().then(|| Collider {
            unique_id: actor.unique_id,
            bounds: actor.movement_collision_bounds(),
        });
        let previous = self.members.get(&actor.runtime_id).copied();
        if previous == collider {
            return;
        }
        match collider {
            Some(collider) => {
                self.members.insert(actor.runtime_id, collider);
            }
            None => {
                self.members.remove(&actor.runtime_id);
            }
        }
        self.invalidate(actor.runtime_id, previous, collider);
    }

    /// Ends one solid actor lifetime without changing snapshots held by readers.
    pub(super) fn remove(&mut self, runtime: u64) {
        if let Some(previous) = self.members.remove(&runtime) {
            self.invalidate(runtime, Some(previous), None);
        }
    }

    /// Discards the current snapshot only if this member changes its filtered bounds.
    fn invalidate(&mut self, runtime: u64, previous: Option<Collider>, next: Option<Collider>) {
        let cached = self.snapshot.get_mut().expect("collider snapshot lock");
        if let Some(snapshot) = cached.as_ref() {
            let visible = |collider: Option<Collider>| {
                collider
                    .filter(|collider| {
                        runtime != snapshot.mover && Some(collider.unique_id) != snapshot.mount
                    })
                    .and_then(|collider| collider.bounds)
                    .map(|(min, max)| (min.map(f32::to_bits), max.map(f32::to_bits)))
            };
            if visible(previous) != visible(next) {
                *cached = None;
            }
        }
    }

    /// Shares stable runtime-ordered bounds, refreshing both mover and mount filters on demand.
    pub(super) fn snapshot(&self, mover: u64, mount: Option<i64>) -> Arc<[ActorCollisionBounds]> {
        let mut cached = self.snapshot.lock().expect("collider snapshot lock");
        if let Some(snapshot) = cached.as_ref()
            && snapshot.mover == mover
            && snapshot.mount == mount
        {
            return Arc::clone(&snapshot.bounds);
        }
        let bounds = self
            .members
            .iter()
            .inspect(|_| {
                #[cfg(test)]
                self.snapshot_visits
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            })
            .filter(|(runtime, collider)| **runtime != mover && Some(collider.unique_id) != mount)
            .filter_map(|(_, collider)| collider.bounds)
            .collect();
        *cached = Some(Snapshot {
            mover,
            mount,
            bounds,
        });
        Arc::clone(&cached.as_ref().expect("installed collider snapshot").bounds)
    }

    /// Reports candidate membership independently of whether its size metadata is valid.
    #[cfg(test)]
    pub(super) fn contains(&self, runtime: &u64) -> bool {
        self.members.contains_key(runtime)
    }
}

#[cfg(test)]
mod tests;
