//! Converts each immutable actor-bound snapshot once for movement and replay.

use std::sync::{Arc, Mutex};

use client_world::ActorCollisionBounds;

#[derive(Debug)]
struct ConvertedSnapshot {
    source: Arc<[ActorCollisionBounds]>,
    bounds: Arc<[sim::Aabb]>,
}

/// Holds one conversion; source ownership prevents pointer reuse across actor-store resets.
#[derive(Debug, Default)]
pub(crate) struct MovementColliderCache(Mutex<Option<ConvertedSnapshot>>);

impl MovementColliderCache {
    /// Reuses converted storage until the actor owner publishes a different immutable source.
    pub(crate) fn snapshot(&self, source: Arc<[ActorCollisionBounds]>) -> Arc<[sim::Aabb]> {
        let mut cached = self.0.lock().expect("movement collider cache lock");
        if let Some(snapshot) = cached.as_ref()
            && Arc::ptr_eq(&snapshot.source, &source)
        {
            return Arc::clone(&snapshot.bounds);
        }
        let bounds = source
            .iter()
            .map(|&(min, max)| {
                let vector = |v: [f32; 3]| sim::Vec3::new(v[0].into(), v[1].into(), v[2].into());
                sim::Aabb::new(vector(min), vector(max))
            })
            .collect();
        *cached = Some(ConvertedSnapshot { source, bounds });
        Arc::clone(
            &cached
                .as_ref()
                .expect("installed collider conversion")
                .bounds,
        )
    }
}

#[cfg(test)]
mod tests;
