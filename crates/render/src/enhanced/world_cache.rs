//! World-owned state is retired as a whole so new histories share the same lifetime.

use render_scene::enhanced::RenderedWorld;
use std::ops::{Deref, DerefMut};

#[derive(Default)]
pub(crate) struct WorldCache<T> {
    world: Option<RenderedWorld>,
    value: T,
}

impl<T: Default> WorldCache<T> {
    /// Retires all owned state on a session or dimension change, including newly added fields.
    pub(crate) fn enter_world(&mut self, world: Option<RenderedWorld>) -> bool {
        if self.world == world {
            return false;
        }
        self.value = T::default();
        self.world = world;
        true
    }
}

impl<T> WorldCache<T> {
    /// Returns the world that owns the cached state.
    pub(crate) fn world(&self) -> Option<RenderedWorld> {
        self.world
    }
}

impl<T> Deref for WorldCache<T> {
    type Target = T;

    /// Borrows the state owned by the current world.
    fn deref(&self) -> &T {
        &self.value
    }
}

impl<T> DerefMut for WorldCache<T> {
    /// Borrows the state owned by the current world for an update.
    fn deref_mut(&mut self) -> &mut T {
        &mut self.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_changes_retire_the_entire_owned_cache() {
        let mut cache = WorldCache::<Vec<u32>>::default();
        let world = RenderedWorld {
            session_generation: 1,
            dimension: 0,
        };
        for next in [
            Some(world),
            Some(RenderedWorld {
                session_generation: 2,
                ..world
            }),
            Some(RenderedWorld {
                session_generation: 2,
                dimension: 1,
            }),
            None,
        ] {
            cache.push(7);
            assert!(cache.enter_world(next));
            assert!(cache.is_empty());
            assert_eq!(cache.world(), next);
            cache.push(9);
            assert!(!cache.enter_world(next));
            assert_eq!(&**cache, &[9]);
        }
    }
}
