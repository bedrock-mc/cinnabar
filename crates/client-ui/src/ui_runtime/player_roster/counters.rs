//! Bounded native counter snapshots reuse formatted annotations while only counts or styles change.

use std::{collections::HashMap, sync::Arc};
use ui::mod_hud::{CounterAnnotation, TotemCounterStyle};

#[derive(Clone, Debug)]
struct Entry {
    count: u32,
    generation: u64,
    annotation: Arc<CounterAnnotation>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct Counters {
    pub(super) style: Option<TotemCounterStyle>,
    snapshot: Vec<([u8; 16], u32, bool)>,
    scratch: Vec<([u8; 16], u32, bool)>,
    entries: HashMap<[u8; 16], Entry>,
    generation: u64,
}

impl Counters {
    /// Formats only changed positive counts and drops every annotation absent from the current interval.
    pub(super) fn update(
        &mut self,
        style: Option<&TotemCounterStyle>,
        facts: impl Iterator<Item = ([u8; 16], u32, bool)>,
    ) -> bool {
        self.scratch.clear();
        if let Some(style) = style {
            self.scratch.extend(
                facts.filter(|(_, count, local)| *count != 0 && (!local || style.show_self)),
            );
        }
        if self.style.as_ref() == style && self.snapshot == self.scratch {
            return false;
        }
        let changed_style = self.style.as_ref() != style;
        self.style = style.cloned();
        std::mem::swap(&mut self.snapshot, &mut self.scratch);
        self.generation = self.generation.wrapping_add(1);
        if self.generation == 0 {
            self.entries.clear();
        }
        if let Some(style) = style {
            for (uuid, count, _) in &self.snapshot {
                match self.entries.entry(*uuid) {
                    std::collections::hash_map::Entry::Occupied(mut entry) => {
                        let entry = entry.get_mut();
                        if changed_style || entry.count != *count {
                            entry.annotation =
                                Arc::new(style.annotation(*count).expect("positive count"));
                            entry.count = *count;
                        }
                        entry.generation = self.generation;
                    }
                    std::collections::hash_map::Entry::Vacant(entry) => {
                        entry.insert(Entry {
                            count: *count,
                            generation: self.generation,
                            annotation: Arc::new(style.annotation(*count).expect("positive count")),
                        });
                    }
                }
            }
        }
        self.entries
            .retain(|_, entry| entry.generation == self.generation && style.is_some());
        true
    }

    /// Shares an existing annotation without allocating or rebuilding a player's name.
    pub(super) fn annotation(&self, uuid: &[u8; 16]) -> Option<&Arc<CounterAnnotation>> {
        self.entries.get(uuid).map(|entry| &entry.annotation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Same-name players keep independent counts, unchanged players reuse their text, and disable clears both surfaces.
    #[test]
    fn totem_counter_snapshots_reuse_unchanged_entries_and_respect_self_and_disable() {
        let mut cache = Counters::default();
        let mut style = TotemCounterStyle::default();
        let a = [1; 16];
        let b = [2; 16];
        assert!(cache.update(Some(&style), [(a, 2, false), (b, 3, true)].into_iter()));
        let before = Arc::clone(cache.annotation(&a).unwrap());
        assert!(!cache.update(Some(&style), [(a, 2, false), (b, 3, true)].into_iter()));
        assert!(cache.update(Some(&style), [(a, 2, false), (b, 4, true)].into_iter()));
        assert!(Arc::ptr_eq(&before, cache.annotation(&a).unwrap()));
        style.show_self = false;
        cache.update(Some(&style), [(a, 2, false), (b, 4, true)].into_iter());
        assert!(cache.annotation(&b).is_none());
        cache.update(None, [(a, 2, false)].into_iter());
        assert!(cache.annotation(&a).is_none());
        cache.update(Some(&style), [(a, 0, false)].into_iter());
        assert!(cache.annotation(&a).is_none());
    }
}
