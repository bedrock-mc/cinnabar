//! Server pose, Riptide spin and item-use flags, modelled on vanilla's actor
//! flag component and its frame corrections.
//!
//! Every completed tick captures the flag word it started with. A server write
//! stamped at a retained tick becomes a correction of the following frame: the
//! bits that differ from that frame's capture, applied to the live state at
//! once and again whenever replay passes the frame. Replay keeps the capture of
//! the frame it starts from and refreshes every later capture from the replayed
//! state before re-applying that frame's corrections. A write stamped at the
//! current or a future tick is queued for the next frame's capture; an
//! unstamped write only changes the live word. The local systems then read the
//! word each tick: pose rules continue or end a pose, the spin runs its
//! lifetime, and item use reconciles with the word before the tick simulates.
//! Flag writes are not start or stop actions, so each input reports edges
//! relative to the word its tick started with.

use super::*;

/// Pose modes in the order a start prefers when one word sets several.
const POSES: [sim::MovementMode; 3] = [
    sim::MovementMode::Gliding,
    sim::MovementMode::Swimming,
    sim::MovementMode::Crawling,
];
/// Flag word bits after the three pose bits, which follow [`POSES`].
const SPINNING: u8 = 1 << 3;
const USING_ITEM: u8 = 1 << 4;

/// The client-owned bits of the actor flag word: glide, swim and crawl poses,
/// the Riptide spin (`DamageNearbyMobs`) and item use.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct FlagWord(u8);

impl FlagWord {
    /// The word the local state forms.
    pub(super) fn capture(mode: sim::MovementMode, spinning: bool, using_item: bool) -> Self {
        let pose = POSES
            .iter()
            .position(|pose| *pose == mode)
            .map_or(0, |index| 1 << index);
        Self(pose | if spinning { SPINNING } else { 0 } | if using_item { USING_ITEM } else { 0 })
    }

    fn has(self, bit: u8) -> bool {
        self.0 & bit != 0
    }

    pub(super) fn using_item(self) -> bool {
        self.has(USING_ITEM)
    }
}

/// The bits one server write carried, and their values.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct FlagWrite {
    present: u8,
    value: u8,
}

impl FlagWrite {
    pub(super) fn from_flags(flags: &client_world::MovementFlagUpdate) -> Self {
        let mut write = Self::default();
        for (bit, flag) in [
            (1, flags.gliding),
            (1 << 1, flags.swimming),
            (1 << 2, flags.crawling),
            (SPINNING, flags.spinning),
            (USING_ITEM, flags.using_item),
        ] {
            if let Some(set) = flag {
                write.present |= bit;
                write.value |= if set { bit } else { 0 };
            }
        }
        write
    }
}

/// Bits a server write changes and their values. Vanilla applies these
/// selected flags only where the write differs from the word it compares with.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct FlagCorrection {
    mask: u8,
    value: u8,
}

impl FlagCorrection {
    /// The bits of `write` that differ from `base`.
    fn against(write: FlagWrite, base: FlagWord) -> Self {
        let mask = write.present & (write.value ^ base.0);
        Self {
            mask,
            value: write.value & mask,
        }
    }

    fn is_empty(self) -> bool {
        self.mask == 0
    }

    /// This correction followed by `later`, whose bits win.
    fn then(self, later: Self) -> Self {
        Self {
            mask: self.mask | later.mask,
            value: (self.value & !later.mask) | later.value,
        }
    }

    /// `word` with this correction applied.
    pub(super) fn apply(self, word: FlagWord) -> FlagWord {
        FlagWord((word.0 & !self.mask) | self.value)
    }
}

/// Ends or starts `modes`' pose so it follows the pose bits of `word` that
/// `mask` covers: a cleared pose ends, and a set pose starts unless a pose the
/// word keeps continues. Cinnabar keeps one pose, so a word setting several
/// starts the first, and flight, which vanilla keeps beside pose flags, stays.
fn apply_pose(word: FlagWord, mask: u8, modes: &mut ModeTracker) {
    let current = modes.mode();
    if current == sim::MovementMode::Flying {
        return;
    }
    let bit = |index: usize| (mask & (1 << index) != 0).then(|| word.has(1 << index));
    let ended = POSES
        .iter()
        .enumerate()
        .any(|(index, pose)| *pose == current && bit(index) == Some(false));
    if ended {
        modes.end(current);
    }
    if POSES.contains(&modes.mode()) {
        return;
    }
    if let Some(pose) = (0..POSES.len())
        .find(|index| bit(*index) == Some(true))
        .map(|index| POSES[index])
    {
        modes.start(pose);
    }
}

/// The word's spin on replayed state; a new spin counts its lifetime from
/// zero, as vanilla's post-spin counter restarts after a clear flag.
fn set_spin(state: &mut PlayerState, spinning: bool) {
    if state.spin_attack_ticks.is_some() != spinning {
        state.spin_attack_ticks = spinning.then_some(0);
    }
}

/// Imposes a frame's effective word on replayed pose and spin.
pub(super) fn impose(word: FlagWord, modes: &mut ModeTracker, state: &mut PlayerState) {
    apply_pose(word, 0b111, modes);
    set_spin(state, word.has(SPINNING));
}

/// Server flag state outside the retained frames.
#[derive(Debug, Clone, Default)]
pub(super) struct ServerFlagAuthority {
    /// A correction stamped at the current or a future tick, attached to the
    /// frame of the given tick when that frame is captured.
    queued: Option<(u64, FlagCorrection)>,
    /// Item-use bit a server write left in the live word, for the item-use
    /// owner to reconcile with on its next tick.
    pending_using_item: Option<bool>,
    /// A spin counter a live clear removed, with the tick it was cleared on.
    /// Vanilla's counter outlives its flag until a tick processes the clear.
    cleared_spin: Option<(u64, u8)>,
}

impl ServerFlagAuthority {
    /// Drops writes that belong to discarded frames.
    pub(super) fn reset_history(&mut self) {
        self.queued = None;
        self.cleared_spin = None;
    }
}

/// Writes the live spin flag; a set restores a counter that a clear on the
/// same tick removed.
fn write_live_spin(state: &mut PlayerState, authority: &mut ServerFlagAuthority, spinning: bool) {
    if state.spin_attack_ticks.is_some() == spinning {
        return;
    }
    if spinning {
        let restored = authority
            .cleared_spin
            .filter(|(tick, _)| *tick == state.tick)
            .map_or(0, |(_, ticks)| ticks);
        state.spin_attack_ticks = Some(restored);
    } else {
        authority.cleared_spin = state.spin_attack_ticks.map(|ticks| (state.tick, ticks));
        state.spin_attack_ticks = None;
    }
}

/// Applies a correction's pose and spin bits to the live state whose word is
/// `live`; vanilla writes the same bits into the live actor before deciding
/// any replay. Its item-use bit reaches the item-use owner when written.
fn apply_live_correction(
    correction: FlagCorrection,
    live: FlagWord,
    modes: &mut ModeTracker,
    state: &mut PlayerState,
    authority: &mut ServerFlagAuthority,
) {
    let word = correction.apply(live);
    apply_pose(word, correction.mask, modes);
    if correction.mask & SPINNING != 0 {
        write_live_spin(state, authority, word.has(SPINNING));
    }
}

/// Captures the live word a fixed tick starts with (`using_item` is the
/// tick's item use), then attaches and applies a correction queued for it; the
/// queue keeps it until a later tick, so a failed tick retries with it. The
/// pose and spin the tick starts from become its action baseline. Returns the
/// capture and the attached correction.
pub(super) fn capture_tick(
    authority: &mut ServerFlagAuthority,
    modes: &mut ModeTracker,
    state: &mut PlayerState,
    input: &mut super::super::TickInput,
    using_item: bool,
) -> (FlagWord, FlagCorrection) {
    let entry = FlagWord::capture(modes.mode(), state.spin_attack_ticks.is_some(), using_item);
    let next = state.tick.checked_add(1);
    authority.queued = authority.queued.filter(|(tick, _)| Some(*tick) >= next);
    let correction = authority
        .queued
        .filter(|(tick, _)| Some(*tick) == next)
        .map_or_else(FlagCorrection::default, |(_, correction)| correction);
    let live = FlagWord::capture(
        modes.mode(),
        state.spin_attack_ticks.is_some(),
        authority.pending_using_item.unwrap_or(using_item),
    );
    apply_live_correction(correction, live, modes, state, authority);
    input.server_mode = Some(modes.mode());
    input.server_spinning = Some(state.spin_attack_ticks.is_some());
    (entry, correction)
}

impl LocalPhysicsController {
    /// The live flag word, including server writes not yet reconciled.
    fn live_flag_word(&self) -> FlagWord {
        let using = self.server_flags.pending_using_item.unwrap_or_else(|| {
            self.controller_history
                .back()
                .is_some_and(|frame| frame.entry_flags.using_item())
        });
        FlagWord::capture(
            self.modes.mode(),
            self.state
                .as_ref()
                .is_some_and(|state| state.spin_attack_ticks.is_some()),
            using,
        )
    }

    /// Applies `correction` to the live state and hands its item-use bit to
    /// the item-use owner.
    fn correct_live(&mut self, correction: FlagCorrection) {
        if correction.mask & USING_ITEM != 0 {
            self.server_flags.pending_using_item = Some(correction.value & USING_ITEM != 0);
        }
        let live = self.live_flag_word();
        if let Some(state) = self.state.as_mut() {
            apply_live_correction(
                correction,
                live,
                &mut self.modes,
                state,
                &mut self.server_flags,
            );
        }
    }

    /// Applies a write stamped `tick` in the live slot: its difference from the
    /// live word changes the live state, and a nonzero stamp also queues it
    /// for the next frame's capture so replay keeps it.
    pub(super) fn adopt_live_server_flags(&mut self, tick: u64, write: FlagWrite) {
        // The item-use owner compares a live item-use write with its own use.
        if write.present & USING_ITEM != 0 {
            self.server_flags.pending_using_item = Some(write.value & USING_ITEM != 0);
        }
        let correction = FlagCorrection::against(write, self.live_flag_word());
        if correction.is_empty() {
            return;
        }
        self.correct_live(correction);
        if tick == 0 {
            return;
        }
        let Some(next) = self
            .state
            .as_ref()
            .and_then(|state| state.tick.checked_add(1))
        else {
            return;
        };
        let queued = match self.server_flags.queued {
            Some((queued_tick, earlier)) if queued_tick == next => earlier.then(correction),
            _ => correction,
        };
        self.server_flags.queued = Some((next, queued));
    }

    /// Attaches a write stamped at retained `tick` to the following frame as
    /// the difference from that frame's capture, and applies it live; returns
    /// whether the frame changed and needs replay.
    pub(super) fn retain_server_flags(&mut self, tick: u64, write: FlagWrite) -> bool {
        let Some(frame) = self
            .controller_history
            .iter_mut()
            .find(|frame| Some(frame.tick) == tick.checked_add(1))
        else {
            return false;
        };
        let correction = FlagCorrection::against(write, frame.entry_flags);
        if correction.is_empty() {
            return false;
        }
        frame.flag_correction = frame.flag_correction.then(correction);
        self.correct_live(correction);
        true
    }

    /// Re-applies a queued correction to a live state that replay rebuilt.
    pub(super) fn reapply_queued_server_flags(&mut self) {
        let next = self
            .state
            .as_ref()
            .and_then(|state| state.tick.checked_add(1));
        if let Some((tick, correction)) = self.server_flags.queued
            && Some(tick) == next
        {
            let live = self.live_flag_word();
            if let Some(state) = self.state.as_mut() {
                apply_live_correction(
                    correction,
                    live,
                    &mut self.modes,
                    state,
                    &mut self.server_flags,
                );
            }
        }
    }

    /// Discards server writes waiting for the next tick when respawn or a
    /// dimension change replaces the old life or world.
    pub fn discard_server_flag_writes(&mut self) {
        self.server_flags.queued = None;
        self.server_flags.pending_using_item = None;
    }

    /// Takes the item-use bit a server write left in the live word.
    pub fn take_server_using_item(&mut self) -> Option<bool> {
        self.server_flags.pending_using_item.take()
    }
}
