use std::collections::BTreeMap;

/// One authored effect occurrence within a completed controller-state entry.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ActorEffectKey {
    pub session_id: u64,
    pub dimension: i32,
    pub runtime_id: u64,
    pub spawn_revision: u64,
    pub reset_generation: u64,
    pub controller: u32,
    pub state: u32,
    pub entered_tick: u64,
    pub effect_index: u16,
}

#[derive(Debug)]
struct ActiveEmitter {
    id: Option<u64>,
    seen: bool,
}

/// Retains an emitter once per state entry, including effects that already completed naturally.
#[derive(Debug, Default)]
pub struct ActorEffectTracker {
    active: BTreeMap<ActorEffectKey, ActiveEmitter>,
}

impl ActorEffectTracker {
    pub fn clear(&mut self) {
        self.active.clear();
    }

    pub fn begin_frame(&mut self) {
        for emitter in self.active.values_mut() {
            emitter.seen = false;
        }
    }

    /// Returns true only for a new state-entry effect; unchanged input starts no emitter.
    pub fn admit(&mut self, key: ActorEffectKey) -> bool {
        if let Some(emitter) = self.active.get_mut(&key) {
            emitter.seen = true;
            return false;
        }
        if self.active.len() >= crate::system::MAX_EMITTERS {
            return false;
        }
        self.active.insert(
            key,
            ActiveEmitter {
                id: None,
                seen: true,
            },
        );
        true
    }

    pub fn started(&mut self, key: ActorEffectKey, id: Option<u64>) {
        if let Some(emitter) = self.active.get_mut(&key) {
            emitter.id = id;
        }
    }

    /// Ends emission on state exit or actor replacement; existing particles finish naturally.
    pub fn finish_frame(&mut self, mut stop: impl FnMut(u64)) {
        self.active.retain(|_, emitter| {
            if !emitter.seen {
                if let Some(id) = emitter.id {
                    stop(id);
                }
                return false;
            }
            true
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> ActorEffectKey {
        ActorEffectKey {
            session_id: 1,
            dimension: 0,
            runtime_id: 4,
            spawn_revision: 1,
            reset_generation: 1,
            controller: 2,
            state: 0,
            entered_tick: 0,
            effect_index: 0,
        }
    }

    #[test]
    fn controller_effect_persists_once_and_stops_on_exit_or_actor_replacement() {
        let mut tracker = ActorEffectTracker::default();
        let mut stopped = Vec::new();
        tracker.begin_frame();
        assert!(tracker.admit(key()));
        tracker.started(key(), Some(10));
        tracker.finish_frame(|id| stopped.push(id));
        tracker.begin_frame();
        assert!(!tracker.admit(key()));
        tracker.finish_frame(|id| stopped.push(id));
        assert!(stopped.is_empty());
        tracker.begin_frame();
        let replacement = ActorEffectKey {
            spawn_revision: 2,
            ..key()
        };
        assert!(tracker.admit(replacement));
        tracker.started(replacement, Some(11));
        tracker.finish_frame(|id| stopped.push(id));
        assert_eq!(stopped, [10]);
        tracker.begin_frame();
        tracker.finish_frame(|id| stopped.push(id));
        assert_eq!(stopped, [10, 11]);
    }

    #[test]
    fn completed_or_rejected_effect_does_not_restart_until_another_state_entry() {
        let mut tracker = ActorEffectTracker::default();
        assert!(tracker.admit(key()));
        tracker.started(key(), None);
        tracker.begin_frame();
        assert!(!tracker.admit(key()));
        tracker.finish_frame(|_| panic!("there is no live emitter to stop"));
        let entered = ActorEffectKey {
            entered_tick: 40,
            ..key()
        };
        tracker.begin_frame();
        assert!(tracker.admit(entered));
        tracker.finish_frame(|_| panic!("there is no live emitter to stop"));
        tracker.clear();
        assert!(tracker.admit(entered));
    }
}
