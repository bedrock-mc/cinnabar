//! Server pose and Riptide spin flags, modelled on vanilla's actor flag
//! component and its frame corrections.
//!
//! Every completed tick captures the flag word it started with. A server write
//! stamped at a retained tick becomes a correction of the following frame: the
//! bits that differ from that frame's capture, applied to the live state at
//! once and again whenever replay passes the frame. Replay keeps the capture of
//! the frame it starts from and refreshes every later capture from the replayed
//! state before re-applying that frame's corrections; the replayed word then
//! replaces the live one, as vanilla copies every client-owned flag back. A
//! write stamped at the current tick is queued for the next frame's capture,
//! one stamped at a future tick joins the current frame's corrections, and an
//! unstamped write only changes the live word. Pose rules then continue or end
//! a pose and the spin runs its lifetime. Flag writes are not start or stop
//! actions, so each input reports edges relative to the word its tick started
//! with. Server item-use writes are not adopted yet.

use super::*;

/// Pose modes in the order a start prefers when one word sets several.
const POSES: [sim::MovementMode; 3] = [
    sim::MovementMode::Gliding,
    sim::MovementMode::Swimming,
    sim::MovementMode::Crawling,
];
/// Flag word bits after the three pose bits, which follow [`POSES`].
const SPINNING: u8 = 1 << 3;

/// The client-owned bits of the actor flag word this module adopts: glide,
/// swim and crawl poses and the Riptide spin (`DamageNearbyMobs`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct FlagWord(u8);

impl FlagWord {
    /// The word the local state forms.
    pub(super) fn capture(mode: sim::MovementMode, spinning: bool) -> Self {
        let pose = POSES
            .iter()
            .position(|pose| *pose == mode)
            .map_or(0, |index| 1 << index);
        Self(pose | if spinning { SPINNING } else { 0 })
    }

    fn has(self, bit: u8) -> bool {
        self.0 & bit != 0
    }

    /// Whether the word's Riptide spin flag is set.
    pub(super) fn spinning(self) -> bool {
        self.has(SPINNING)
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
/// A changed pose also becomes the swimming-or-crawling flag the next tick's
/// swim blend reads in `state`.
fn apply_pose(word: FlagWord, mask: u8, modes: &mut ModeTracker, state: &mut PlayerState) {
    let current = modes.mode();
    write_pose(word, mask, modes);
    if modes.mode() != current {
        state.swim_pose_active = matches!(
            modes.mode(),
            sim::MovementMode::Swimming | sim::MovementMode::Crawling
        );
    }
}

/// The pose half of [`apply_pose`].
fn write_pose(word: FlagWord, mask: u8, modes: &mut ModeTracker) {
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
fn set_spin(state: &mut PlayerState, spinning: bool, dormant: u8) {
    if state.spin_attack_ticks.is_some() != spinning {
        state.spin_attack_ticks = spinning.then_some(dormant);
    }
}

/// Imposes a frame's effective word on replayed pose and spin; a spin the word
/// starts resumes from the `dormant` counter the previous tick left.
pub(super) fn impose(
    word: FlagWord,
    modes: &mut ModeTracker,
    state: &mut PlayerState,
    dormant: u8,
) {
    apply_pose(word, 0b111, modes, state);
    set_spin(state, word.has(SPINNING), dormant);
}

/// The inactive spin counter a tick leaves in `state` when a spin was active
/// during it (`spun`). Vanilla's post-spin step zeroes the counter on a side
/// collision and then counts the stopping tick, so it is one until the next
/// tick processes the clear flag; every other inactive counter is zero.
pub(super) fn dormant_spin(spun: bool, state: &PlayerState) -> u8 {
    u8::from(
        spun && state.spin_attack_ticks.is_none() && (state.collisions.x || state.collisions.z),
    )
}

/// Counters vanilla keeps in components beside the flag word and snapshots
/// with every frame: the glide counter, the swim blend and the Riptide spin
/// counter, which keeps its value while the spin flag is clear.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(super) struct EntryCounters {
    fall_fly_ticks: u32,
    swim_amount: f32,
    spin: u8,
}

impl EntryCounters {
    /// The counters `modes` and `state` hold; `dormant` is the spin counter
    /// while the spin flag is clear.
    pub(super) fn capture(modes: &ModeTracker, state: &PlayerState, dormant: u8) -> Self {
        Self {
            fall_fly_ticks: modes.fall_fly_ticks(),
            swim_amount: state.swim_amount,
            spin: state.spin_attack_ticks.unwrap_or(dormant),
        }
    }

    /// Restores these counters as the state a replay starts from, as vanilla
    /// restores the starting frame's snapshot; returns the spin counter a spin
    /// the frame's word starts resumes from.
    pub(super) fn restore(self, modes: &mut ModeTracker, state: &mut PlayerState) -> u8 {
        modes.set_glide_counter(self.fall_fly_ticks);
        state.swim_amount = self.swim_amount;
        if let Some(ticks) = state.spin_attack_ticks.as_mut() {
            *ticks = self.spin;
        }
        self.spin
    }
}

/// Server flag state outside the retained frames.
#[derive(Debug, Clone, Default)]
pub(super) struct ServerFlagAuthority {
    /// A correction stamped at the current or a future tick, attached to the
    /// frame of the given tick when that frame is captured.
    queued: Option<(u64, FlagCorrection)>,
    /// A spin counter a live clear removed, with the tick it was cleared on.
    /// Vanilla's counter outlives its flag until a tick processes the clear.
    cleared_spin: Option<(u64, u8)>,
}

impl ServerFlagAuthority {
    /// The spin counter a live clear on `tick` removed; it still counts until
    /// a tick processes the clear.
    pub(super) fn cleared_spin_on(&self, tick: u64) -> Option<u8> {
        self.cleared_spin
            .filter(|(cleared, _)| *cleared == tick)
            .map(|(_, ticks)| ticks)
    }

    /// Saves the counter of a spin whose flag was cleared on `tick`, for a set
    /// on the same tick to resume.
    pub(super) fn save_cleared_spin(&mut self, tick: u64, ticks: u8) {
        self.cleared_spin = Some((tick, ticks));
    }

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
        state.spin_attack_ticks = Some(authority.cleared_spin_on(state.tick).unwrap_or(0));
    } else if let Some(ticks) = state.spin_attack_ticks.take() {
        authority.save_cleared_spin(state.tick, ticks);
    }
}

/// Records the counter of a spin that `spun` during the tick, whether it
/// started from the tick's entry word or the tick launched it, and that the
/// tick ended by a side collision. Vanilla's post-spin step zeroes the counter
/// and then counts the stopping tick, so it stays at one until the next tick
/// processes the clear flag; a server set before then resumes from it.
pub(super) fn note_spin_end(authority: &mut ServerFlagAuthority, spun: bool, state: &PlayerState) {
    let dormant = dormant_spin(spun, state);
    if dormant > 0 {
        authority.save_cleared_spin(state.tick, dormant);
    }
}

/// Applies a correction's pose and spin bits to the live state whose word is
/// `live`; vanilla writes the same bits into the live actor before deciding
/// any replay.
fn apply_live_correction(
    correction: FlagCorrection,
    live: FlagWord,
    modes: &mut ModeTracker,
    state: &mut PlayerState,
    authority: &mut ServerFlagAuthority,
) {
    let word = correction.apply(live);
    apply_pose(word, correction.mask, modes, state);
    if correction.mask & SPINNING != 0 {
        write_live_spin(state, authority, word.has(SPINNING));
    }
}

/// Captures the live word a fixed tick starts with and attaches a correction
/// queued for it without applying it again, as vanilla moves queued corrections into the captured frame; the
/// queue keeps it until a later tick, so a failed tick retries with it. The
/// pose and spin the tick starts from become its action baseline. Returns the
/// capture, the attached correction and the counters the tick starts with.
pub(super) fn capture_tick(
    authority: &mut ServerFlagAuthority,
    modes: &ModeTracker,
    state: &PlayerState,
    input: &mut super::super::TickInput,
) -> (FlagWord, FlagCorrection, EntryCounters) {
    let entry = FlagWord::capture(modes.mode(), state.spin_attack_ticks.is_some());
    let dormant = authority.cleared_spin_on(state.tick).unwrap_or(0);
    let counters = EntryCounters::capture(modes, state, dormant);
    let next = state.tick.checked_add(1);
    authority.queued = authority.queued.filter(|(tick, _)| Some(*tick) >= next);
    let correction = authority
        .queued
        .filter(|(tick, _)| Some(*tick) == next)
        .map_or_else(FlagCorrection::default, |(_, correction)| correction);
    input.server_mode = Some(modes.mode());
    input.server_spinning = Some(state.spin_attack_ticks.is_some());
    (entry, correction, counters)
}

impl LocalPhysicsController {
    /// The live flag word.
    fn live_flag_word(&self) -> FlagWord {
        FlagWord::capture(
            self.modes.mode(),
            self.state
                .as_ref()
                .is_some_and(|state| state.spin_attack_ticks.is_some()),
        )
    }

    /// Applies `correction` to the live state.
    fn correct_live(&mut self, correction: FlagCorrection) {
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
    /// live word changes the live state. A stamp at the current tick, or an
    /// earlier one without a following frame, also queues it for the next
    /// frame's capture, and a future stamp, whose frame
    /// does not exist, joins the current frame's corrections, so replay keeps
    /// it either way.
    pub(super) fn adopt_live_server_flags(&mut self, tick: u64, write: FlagWrite) {
        let correction = FlagCorrection::against(write, self.live_flag_word());
        if correction.is_empty() {
            return;
        }
        self.correct_live(correction);
        if tick == 0 {
            return;
        }
        let Some(current) = self.state.as_ref().map(|state| state.tick) else {
            return;
        };
        if tick > current {
            if let Some(frame) = self
                .controller_history
                .iter_mut()
                .find(|frame| frame.tick == current)
            {
                frame.flag_correction = frame.flag_correction.then(correction);
            }
            return;
        }
        let Some(next) = current.checked_add(1) else {
            return;
        };
        let queued = match self.server_flags.queued {
            Some((queued_tick, earlier)) if queued_tick == next => earlier.then(correction),
            _ => correction,
        };
        self.server_flags.queued = Some((next, queued));
    }

    /// Attaches a write stamped at retained `tick` to the following frame as
    /// the difference from that frame's capture, and applies it live. Like
    /// vanilla's flag correction, whose pending type is zero, it schedules no
    /// replay; a later replay through the frame applies it. Without a
    /// following frame, as when a stale stamp clamps to the only retained
    /// tick, the write applies live and waits for the next capture. Returns
    /// whether a retained frame changed.
    pub(super) fn retain_server_flags(&mut self, tick: u64, write: FlagWrite) -> bool {
        let Some(frame) = self
            .controller_history
            .iter_mut()
            .find(|frame| Some(frame.tick) == tick.checked_add(1))
        else {
            self.adopt_live_server_flags(tick, write);
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

    /// Discards server writes waiting for the next tick when respawn or a
    /// dimension change replaces the old life or world, and ends the old
    /// life's Riptide spin with its saved counter; the spatial snap that
    /// follows keeps flag state as it finds it.
    pub fn discard_server_flag_writes(&mut self) {
        self.server_flags.queued = None;
        self.server_flags.cleared_spin = None;
        if let Some(state) = self.state.as_mut() {
            state.spin_attack_ticks = None;
        }
    }
}
