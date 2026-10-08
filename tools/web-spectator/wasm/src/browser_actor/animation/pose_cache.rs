//! One converted pose pair per actor; fixed-tick generations own invalidation.
use std::collections::HashMap;

// Rig, animated tick/reset, then actual observed tick/rest reset. Budget freezes
// replace previous with current while retaining the animated generations.
type PoseKey = (u32, u64, u64, u64, u64);

#[derive(Default)]
pub(super) struct PoseCache<T> {
    entries: HashMap<u64, (PoseKey, T)>,
}

impl<T> PoseCache<T> {
    pub(super) fn clear(&mut self) {
        self.entries.clear();
    }
    pub(super) fn invalidate(&mut self, actor: u64) {
        self.entries.remove(&actor);
    }

    pub(super) fn get_or_insert_with(
        &mut self,
        actor: u64,
        key: PoseKey,
        convert: impl FnOnce() -> Option<T>,
    ) -> Option<&T> {
        if self.entries.get(&actor).is_none_or(|entry| entry.0 != key) {
            self.entries.insert(actor, (key, convert()?));
        }
        self.entries.get(&actor).map(|entry| &entry.1)
    }
}

#[cfg(test)]
mod tests {
    use super::PoseCache;
    use std::sync::Arc;

    #[test]
    fn render_frames_share_arcs_until_native_pose_or_lifetime_changes() {
        let mut cache = PoseCache::default();
        let key = (1, 1, 1, 1, 1);
        let first = cache
            .get_or_insert_with(7, key, || Some(Arc::new([1])))
            .unwrap()
            .clone();
        let same = cache
            .get_or_insert_with(7, key, || panic!("unchanged pose must not convert"))
            .unwrap();
        assert!(Arc::ptr_eq(&first, same));
        let mut last = first;
        for key in [
            (1, 2, 1, 1, 1),
            (1, 2, 2, 1, 1),
            (2, 2, 2, 1, 1),
            (2, 2, 2, 1, 2),
        ] {
            let next = cache
                .get_or_insert_with(7, key, || Some(Arc::new([1])))
                .unwrap()
                .clone();
            assert_eq!(*last, *next);
            assert!(!Arc::ptr_eq(&last, &next));
            last = next;
        }
        cache.invalidate(7); // Removed actor or a replaced skin.
        let replacement = cache
            .get_or_insert_with(7, (2, 2, 2, 1, 2), || Some(Arc::new([1])))
            .unwrap()
            .clone();
        assert!(!Arc::ptr_eq(&last, &replacement));
        cache.clear(); // Another session may reuse the same runtime/rig identifiers.
        let session = cache
            .get_or_insert_with(7, (2, 2, 2, 1, 2), || Some(Arc::new([1])))
            .unwrap();
        assert!(!Arc::ptr_eq(&replacement, session));
    }

    #[test]
    fn failed_conversion_never_publishes_an_old_or_incomplete_pair() {
        let mut cache = PoseCache::default();
        cache
            .get_or_insert_with(7, (1, 1, 1, 1, 1), || Some([1, 2]))
            .unwrap();
        assert!(
            cache
                .get_or_insert_with(7, (1, 2, 1, 2, 1), || None)
                .is_none()
        );
        assert_eq!(
            cache.get_or_insert_with(7, (1, 2, 1, 2, 1), || Some([3, 4])),
            Some(&[3, 4])
        );
    }

    #[test]
    fn frozen_tick_after_success_holds_current_instead_of_replaying_previous() {
        let mut cache = PoseCache::default();
        let animated = (Arc::new([0]), Arc::new([1]));
        let successful = cache
            .get_or_insert_with(7, (1, 10, 1, 10, 1), || Some(animated.clone()))
            .unwrap();
        assert_eq!(*successful.0, [0]);
        assert_eq!(*successful.1, [1]);
        // Native budget/invalid branches preserve animated tick/reset but observe
        // tick 11 and replace previous with current before publishing the snapshot.
        let frozen = cache
            .get_or_insert_with(7, (1, 10, 1, 11, 1), || {
                Some((Arc::clone(&animated.1), Arc::clone(&animated.1)))
            })
            .unwrap();
        assert_eq!(*frozen.0, [1]);
        assert!(Arc::ptr_eq(&frozen.0, &frozen.1));
        let held = Arc::clone(&frozen.0);
        let same = cache
            .get_or_insert_with(7, (1, 10, 1, 11, 1), || {
                panic!("same frozen tick must reuse")
            })
            .unwrap();
        assert!(Arc::ptr_eq(&held, &same.0));
    }
}
