use super::{java_swing_duration, swing_duration};
use crate::movement::{LocalMovementEffectTimeline, MAX_LOCAL_PHYSICS_TICKS_PER_FRAME};
use client_world::LocalSwingProgress;

/// Packet admission and both animation counters on the committed local simulation clock.
#[derive(Debug, Default, Clone)]
pub struct SwingTracker {
    authority: Option<(u64, u64)>,
    completed_tick: Option<u64>,
    attempted_tick: Option<u64>,
    deferred_attempt: Option<(u64, [i32; 2])>,
    prior_tick_states: [Counter; 2],
    started: Option<i32>,
    states: [Counter; 2],
    history: [(i32, i32); MAX_LOCAL_PHYSICS_TICKS_PER_FRAME],
    guard_history: [i32; MAX_LOCAL_PHYSICS_TICKS_PER_FRAME],
    history_end: Option<u64>,
    history_len: usize,
    progress_history: [Option<(u64, LocalSwingProgress)>; MAX_LOCAL_PHYSICS_TICKS_PER_FRAME],
}

#[derive(Debug, Clone)]
struct Counter {
    counter: Option<i32>,
    progress: [f32; 2],
    duration: i32,
}

impl Default for Counter {
    /// Idle counters use the shared native base duration.
    fn default() -> Self {
        Self {
            counter: None,
            progress: [0.0; 2],
            duration: client_world::ACTOR_SWING_TICKS,
        }
    }
}

impl Counter {
    /// Admission reads the counter before this tick's increment.
    fn try_start(&mut self, duration: i32) -> bool {
        let accepted = self
            .counter
            .is_none_or(|counter| counter < 0 || counter >= duration.max(1) / 2);
        if accepted {
            self.counter = Some(-1);
        }
        accepted
    }

    /// Skips an interval with one denominator while retaining the final two native samples.
    fn advance(&mut self, count: u64, duration: i32) {
        if count == 0 {
            return;
        }
        self.duration = duration.max(1);
        let counter = self.counter;
        let value = |steps: u64| {
            counter.and_then(|counter| {
                let next = i128::from(counter) + i128::from(steps);
                (next < i128::from(self.duration)).then_some(next as i32)
            })
        };
        let previous = if count == 1 {
            self.progress[1]
        } else {
            value(count - 1).map_or(0.0, |counter| counter.max(0) as f32 / self.duration as f32)
        };
        self.counter = value(count);
        self.progress = [
            previous,
            self.counter
                .map_or(0.0, |counter| counter.max(0) as f32 / self.duration as f32),
        ];
    }
}

impl SwingTracker {
    /// Supplies each committed tick's post-expiry denominators and resets on a new movement authority.
    pub fn sync_ticks(
        &mut self,
        authority: (u64, u64),
        completed_tick: u64,
        effects: &LocalMovementEffectTimeline,
    ) {
        if self.authority != Some(authority) {
            *self = Self {
                authority: Some(authority),
                ..Self::default()
            };
        }
        self.history_end = Some(completed_tick);
        self.history_len = effects.recent_tick_count();
        for distance in 0..self.history_len {
            let (before, after) = effects.mining_tick(
                completed_tick.saturating_sub(distance as u64),
                completed_tick,
            );
            self.guard_history[distance] = java_swing_duration(before);
            self.history[distance] = (swing_duration(after), java_swing_duration(after));
        }
    }

    /// Identifies the movement authority owning the current counters and retry history.
    pub fn authority_identity(&self) -> Option<(u64, u64)> {
        self.authority
    }

    /// Reports ticks already consumed by animation publication, independently of retry permission.
    pub fn tick_is_published(&self, tick: u64) -> bool {
        self.completed_tick
            .is_some_and(|completed| tick <= completed)
    }

    /// Identifies the latest consumed tick without granting permission to replay it.
    pub fn tick_is_current_publication(&self, tick: u64) -> bool {
        self.completed_tick == Some(tick)
    }

    /// Attempts both native animations independently; the result admits the Bedrock wire packet.
    pub fn try_swing(&mut self, tick: u64, duration: i32) -> bool {
        if self.authority.is_none() && self.attempted_tick.is_some_and(|previous| tick < previous) {
            *self = Self::default();
        }
        let deferred = self
            .deferred_attempt
            .filter(|(attempt, _)| *attempt == tick);
        let replay = self.completed_tick == Some(tick) && deferred.is_some();
        if self.attempted_tick == Some(tick)
            || self
                .completed_tick
                .is_some_and(|completed| tick < completed || (tick == completed && !replay))
        {
            return false;
        }
        let published_durations = self.states.each_ref().map(|state| state.duration);
        if replay {
            self.states = self.prior_tick_states.clone();
        } else if tick > 0 {
            self.advance_to(tick - 1);
        }
        let duration = deferred
            .filter(|_| replay)
            .map_or(duration, |(_, values)| values[0]);
        let java_duration = deferred.filter(|_| replay).map_or_else(
            || self.guard_java_duration(tick).unwrap_or(duration),
            |(_, values)| values[1],
        );
        let bedrock = self.states[0].try_start(duration);
        self.states[1].try_start(java_duration);
        self.states[0].duration = duration.max(1);
        self.states[1].duration = java_duration.max(1);
        self.attempted_tick = Some(tick);
        self.deferred_attempt = None;
        if replay {
            self.advance_states(1, published_durations);
            self.record_progress(tick);
        }
        if bedrock {
            self.started = Some(duration);
        }
        bedrock
    }

    /// Retains only an attempted admission that was rolled back by a rejected packet batch.
    pub fn defer_unadmitted_attempt(&mut self, candidate: &Self) {
        if self.authority == candidate.authority
            && candidate.attempted_tick != self.attempted_tick
            && let Some(tick) = candidate.attempted_tick
        {
            let durations = self
                .deferred_attempt
                .filter(|(attempt, _)| *attempt == tick)
                .map_or_else(
                    || candidate.states.each_ref().map(|state| state.duration),
                    |(_, durations)| durations,
                );
            self.deferred_attempt = Some((tick, durations));
        }
    }

    /// Returns the latest accepted wire duration for callers that use the scalar start API.
    pub fn take_started(&mut self) -> Option<i32> {
        self.started.take()
    }

    /// Finishes every committed tick before publishing each mode's previous/current samples.
    pub fn published_progress(&mut self, completed_tick: u64) -> LocalSwingProgress {
        self.advance_to(completed_tick);
        self.started = None;
        self.progress()
    }

    /// Retains each recent completed tick's samples for local motion on the same physics clock.
    pub fn committed_samples(&self) -> impl Iterator<Item = (u64, LocalSwingProgress)> + '_ {
        let last = self.completed_tick.unwrap_or(0);
        let first = last.saturating_sub(MAX_LOCAL_PHYSICS_TICKS_PER_FRAME as u64 - 1);
        (first..=last).filter_map(move |tick| {
            self.progress_history[(tick % MAX_LOCAL_PHYSICS_TICKS_PER_FRAME as u64) as usize]
                .filter(|(stored, _)| *stored == tick)
        })
    }

    /// Finds both post-expiry denominators for a tick retained in this frame's effect history.
    fn post_tick_durations(&self, tick: u64) -> Option<[i32; 2]> {
        let distance = self.history_end?.checked_sub(tick)? as usize;
        (distance < self.history_len).then(|| {
            let durations = self.history[distance];
            [durations.0, durations.1]
        })
    }

    /// The pre-expiry Java duration is retained alongside the post-expiry animation history.
    fn guard_java_duration(&self, tick: u64) -> Option<i32> {
        let distance = self.history_end?.checked_sub(tick)? as usize;
        (distance < self.history_len).then(|| self.guard_history[distance])
    }

    /// Advances only unconsumed local ticks, with bounded work even after a large tick jump.
    fn advance_to(&mut self, target: u64) {
        if self
            .completed_tick
            .is_some_and(|completed| target <= completed)
        {
            return;
        }
        let start = self
            .completed_tick
            .and_then(|tick| tick.checked_add(1))
            .or(self.attempted_tick)
            .or_else(|| {
                self.history_end
                    .filter(|_| self.history_len > 0)
                    .map(|tick| tick.saturating_sub(self.history_len as u64 - 1))
            });
        let Some(mut next) = start else {
            let fallback = self
                .deferred_attempt
                .filter(|(tick, _)| *tick == target)
                .map_or_else(
                    || self.states.each_ref().map(|state| state.duration),
                    |(_, durations)| durations,
                );
            self.advance_states(1, self.post_tick_durations(target).unwrap_or(fallback));
            self.record_progress(target);
            self.completed_tick = Some(target);
            return;
        };
        if next > target {
            return;
        }
        let first = self
            .history_end
            .filter(|_| self.history_len > 0)
            .map(|end| end.saturating_sub(self.history_len as u64 - 1));
        if let Some(first) = first {
            if next < first {
                let last = target.min(first - 1);
                self.advance_recorded(
                    next,
                    last,
                    self.states.each_ref().map(|state| state.duration),
                );
                next = last.saturating_add(1);
            }
            if next <= target {
                let recorded_ticks = next..=target.min(self.history_end.unwrap());
                for tick in recorded_ticks {
                    let distance = (self.history_end.unwrap() - tick) as usize;
                    let durations = self.history[distance];
                    self.advance_states(1, [durations.0, durations.1]);
                    self.record_progress(tick);
                    if tick == target {
                        self.completed_tick = Some(target);
                        return;
                    }
                    next = tick + 1;
                }
            }
        }
        if next <= target {
            self.advance_recorded(
                next,
                target,
                self.states.each_ref().map(|state| state.duration),
            );
        }
        self.completed_tick = Some(target);
    }

    /// Publishes the same committed counters to interpolation and per-tick motion consumers.
    fn progress(&self) -> LocalSwingProgress {
        LocalSwingProgress {
            bedrock: self.states[0].progress,
            java: self.states[1].progress,
            frame_alpha: None,
        }
    }

    /// Replaces a completed tick in the bounded history, including an admitted same-tick retry.
    fn record_progress(&mut self, tick: u64) {
        let index = (tick % MAX_LOCAL_PHYSICS_TICKS_PER_FRAME as u64) as usize;
        self.progress_history[index] = Some((tick, self.progress()));
    }

    /// Skips an old interval in constant work and retains its newest bounded motion samples.
    fn advance_recorded(&mut self, first: u64, last: u64, durations: [i32; 2]) {
        let tail = (last - first).min(MAX_LOCAL_PHYSICS_TICKS_PER_FRAME as u64 - 1);
        let start = last - tail;
        if start > first {
            self.advance_states(start - first, durations);
        }
        for tick in start..=last {
            self.advance_states(1, durations);
            self.record_progress(tick);
        }
    }

    /// Advances a nonempty interval, retaining its last tick's pre-increment counters for a retry.
    fn advance_states(&mut self, count: u64, durations: [i32; 2]) {
        for (state, duration) in self.states.iter_mut().zip(durations) {
            state.advance(count - 1, duration);
        }
        self.prior_tick_states = self.states.clone();
        for (state, duration) in self.states.iter_mut().zip(durations) {
            state.advance(1, duration);
        }
    }
}
