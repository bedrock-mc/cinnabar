//! Outbound transport hand-off for completed physics samples.
//!
//! The bounded retry FIFO lives on [`MovementTicker`]; this module owns the
//! single point where a queued completed sample is encoded, staged as an
//! immutable admission record, and handed to the network transport — or,
//! while the provisional spawn-settle gate suppresses transmission, drained
//! and withheld instead.

pub use protocol::InteractionPacketGuard;
use protocol::{
    Packet, PlayerAuthInputError, player_auth_input_with_interactions,
    player_auth_input_with_mining_request,
};

/// Failure taxonomy of one bounded outbound movement flush.
///
/// Extracted verbatim from the movement root module to respect the per-file
/// architecture line policy; the public path
/// (`crate::movement::MovementSendError`) is unchanged.
#[derive(Debug, PartialEq, Eq)]
pub enum MovementSendError<E> {
    Encode(PlayerAuthInputError),
    Transport(E),
    RestoreOverflow,
    MissingEvidenceContext,
}

/// Terminal reconciliation classification of the outbound physics stream.
///
/// Extracted verbatim from the movement root module to respect the per-file
/// architecture line policy; the public path
/// (`crate::movement::MovementOutboxReconciliation`) is unchanged.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MovementOutboxReconciliation {
    #[default]
    NotAuthoritative,
    Drained,
    SocketPending,
    BudgetDeferred,
    TransportRestored,
    FullRestored,
    /// The outbound stream was healthy when the REMOTE side terminated the
    /// transport mid-session. This is a terminal classification only: it is
    /// latched from any receive-side session failure, including server-initiated
    /// kicks, so it means "not an outbox-drain fault", never client exoneration;
    /// the normalized disconnect reason remains the authority for why the
    /// server hung up.
    RemoteClosed,
}

impl MovementOutboxReconciliation {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotAuthoritative => "NotAuthoritative",
            Self::Drained => "Drained",
            Self::SocketPending => "SocketPending",
            Self::BudgetDeferred => "BudgetDeferred",
            Self::TransportRestored => "TransportRestored",
            Self::FullRestored => "FullRestored",
            Self::RemoteClosed => "RemoteClosed",
        }
    }
}

use super::{
    MovementTicker, PhysicsAuthorityFault, PhysicsSendIdentity, PhysicsTickEvidenceContext,
};

/// A release batch fenced behind the input tick that carries its facing.
#[derive(Debug, Clone)]
pub(super) struct HeldRelease {
    tick: u64,
    packets: Vec<Packet>,
    facing_sent: bool,
}

/// Tick-bound action flags set before their tick was built.
#[derive(Debug, Clone, Copy)]
pub(super) struct NextTickFlags {
    session_generation: u64,
    tick: u64,
    flags: protocol::PlayerInputFlags,
}

/// Recent tick-end states by tick, fed wherever a tick completes, is replayed or is
/// anchored; transport hand-offs never move them.
#[derive(Debug, Clone)]
pub(super) struct TickEnds(std::collections::VecDeque<UnsentSampleView>);

/// The predecessor is the only state read; a few spare entries absorb replays.
const TICK_END_CAPACITY: usize = 8;

impl Default for TickEnds {
    fn default() -> Self {
        Self(std::collections::VecDeque::with_capacity(TICK_END_CAPACITY))
    }
}

impl TickEnds {
    /// Records a tick's end state; it supersedes that tick and any later ones.
    pub(super) fn record(&mut self, view: UnsentSampleView) {
        while self.0.back().is_some_and(|last| last.tick >= view.tick) {
            self.0.pop_back();
        }
        if self.0.len() == TICK_END_CAPACITY {
            self.0.pop_front();
        }
        self.0.push_back(view);
    }

    pub(super) fn get(&self, tick: u64) -> Option<UnsentSampleView> {
        self.0.iter().rev().find(|view| view.tick == tick).copied()
    }

    pub(super) fn clear(&mut self) {
        self.0.clear();
    }
}

/// The reported pose at the end of one completed tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UnsentSampleView {
    pub tick: u64,
    /// Network (eye-offset) position.
    pub position: [f32; 3],
    pub delta: [f32; 3],
    /// Resolved motion controls held-use cadence independently of outbound velocity.
    pub displacement: [f32; 3],
    pub sneaking: bool,
}

impl UnsentSampleView {
    /// Reads the interaction pose without copying queued transport state.
    pub(super) fn from_queued(sample: &super::QueuedPhysicsSample) -> Self {
        Self::from_parts(
            sample.snapshot.tick,
            sample.snapshot.position,
            sample.snapshot.delta,
            sample.displacement,
            sample.snapshot.flags,
        )
    }

    /// Reads a replayed tick with the flags its packet now reports.
    pub(super) fn from_replayed(
        sample: &super::PhysicsMovementSample,
        flags: protocol::PlayerInputFlags,
    ) -> Self {
        Self::from_parts(
            sample.tick,
            sample.position,
            sample.velocity,
            sample.movement,
            flags,
        )
    }

    pub(super) fn from_parts(
        tick: u64,
        position: [f32; 3],
        delta: [f32; 3],
        displacement: [f32; 3],
        flags: protocol::PlayerInputFlags,
    ) -> Self {
        Self {
            tick,
            position,
            delta,
            displacement,
            sneaking: flags.bits() & protocol::PlayerInputFlags::SNEAKING.bits() != 0,
        }
    }
}

/// Where a standalone interaction sits on the movement stream: the tick whose input follows it
/// and the network position it reports.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InteractionSample {
    pub tick: u64,
    pub position: [f32; 3],
}

impl From<UnsentSampleView> for InteractionSample {
    fn from(sample: UnsentSampleView) -> Self {
        Self {
            tick: sample.tick,
            position: sample.position,
        }
    }
}

/// Capacity of every movement retry queue: queued samples, staged sends,
/// sent-history confirmations, and retained tick evidence.
pub const OUTBOX_CAPACITY: usize = 32;

#[cfg(any(test, feature = "test-support"))]
pub fn flush_player_auth_inputs<E>(
    ticker: &mut MovementTicker,
    budget: usize,
    evidence_context: Option<PhysicsTickEvidenceContext>,
    mut send: impl FnMut(PhysicsSendIdentity, Packet) -> Result<(), E>,
) -> Result<usize, MovementSendError<E>> {
    flush_player_auth_inputs_guarded(
        ticker,
        budget,
        evidence_context,
        |identity, packet, _guard| send(identity, packet),
    )
}

pub fn flush_player_auth_inputs_guarded<E>(
    ticker: &mut MovementTicker,
    budget: usize,
    evidence_context: Option<PhysicsTickEvidenceContext>,
    mut send: impl FnMut(PhysicsSendIdentity, Packet, Option<InteractionPacketGuard>) -> Result<(), E>,
) -> Result<usize, MovementSendError<E>> {
    if !ticker.physics_is_authorized() {
        ticker.outbox_reconciliation = MovementOutboxReconciliation::NotAuthoritative;
        return Ok(0);
    }
    if ticker.terminal_drain || ticker.has_unresolved_position_authority_change() {
        ticker.refresh_outbox_reconciliation();
        return Ok(0);
    }
    if !ticker.outbox.is_empty() && evidence_context.is_none() {
        return Err(MovementSendError::MissingEvidenceContext);
    }

    let mut sent = 0;
    for _ in 0..budget {
        let Some(mut sample) = ticker.outbox.front().cloned() else {
            break;
        };
        if ticker
            .held_release
            .as_ref()
            .is_some_and(|held| sample.snapshot.tick > held.tick)
        {
            break;
        }
        if ticker.tick_evidence.len() == OUTBOX_CAPACITY {
            ticker.fail_physics_authority(&PhysicsAuthorityFault::OutboxOverflow);
            break;
        }
        // Retries project current teleport authority. Consume the one-shot
        // acknowledgement only after transport accepts the packet.
        let carried_teleport_ack = ticker.project_pending_teleport_ack(&mut sample);
        let interaction_epoch = sample
            .mining
            .as_ref()
            .map(|_| &ticker.mining_epoch_publisher);
        let interaction_guard = interaction_epoch
            .map(|publisher| {
                let movement_packet = player_auth_input_with_interactions(
                    sample.snapshot,
                    &protocol::PlayerAuthInputInteractions::default(),
                )?;
                Ok::<_, PlayerAuthInputError>(InteractionPacketGuard::new(
                    *publisher.borrow(),
                    publisher.subscribe(),
                    movement_packet,
                ))
            })
            .transpose()
            .map_err(MovementSendError::Encode)?;
        let interactions = sample
            .mining
            .as_ref()
            .map(|mining| mining.interactions.clone())
            .unwrap_or_default();
        let mining_request = sample
            .mining
            .as_ref()
            .and_then(|mining| mining.mining_request);
        let packet =
            player_auth_input_with_mining_request(sample.snapshot, &interactions, mining_request)
                .map_err(MovementSendError::Encode)?;
        ticker
            .pop_pending()
            .expect("encoded sample remains at the FIFO front");
        let identity = ticker.next_send_identity(&sample);
        ticker.note_command_admitted(
            identity,
            sample,
            evidence_context.expect("nonempty outbox requires staged evidence context"),
        );
        if let Err(error) = send(identity, packet, interaction_guard) {
            let sample = ticker
                .restore_admitted(identity)
                .map_err(|_| MovementSendError::RestoreOverflow)?;
            ticker
                .retry_front(sample)
                .map_err(|_| MovementSendError::RestoreOverflow)?;
            ticker.outbox_reconciliation = MovementOutboxReconciliation::TransportRestored;
            return Err(MovementSendError::Transport(error));
        }
        if carried_teleport_ack {
            ticker.consume_pending_teleport_ack();
        }
        sent += 1;
    }
    ticker.refresh_outbox_reconciliation();
    Ok(sent)
}

impl MovementTicker {
    pub fn accepts_block_interactions(&self) -> bool {
        self.physics_is_authorized()
            && !self.terminal_drain
            && !self.has_unresolved_position_authority_change()
    }

    /// Unsent ticks newer than `after`, oldest first, with their post-tick motion.
    pub fn unstepped_interaction_ticks(
        &self,
        after: Option<u64>,
    ) -> Vec<(u64, crate::survival_mining::TickMotion)> {
        self.outbox
            .iter()
            .filter(|sample| after.is_none_or(|after| sample.snapshot.tick > after))
            .map(|sample| {
                let moved = sample
                    .snapshot
                    .delta
                    .iter()
                    .map(|axis| axis * axis)
                    .sum::<f32>();
                let motion = crate::survival_mining::TickMotion {
                    on_ground: sample.evidence.grounded_after_tick,
                    moved: if moved.is_finite() { moved.sqrt() } else { 0.0 },
                };
                (sample.snapshot.tick, motion)
            })
            .collect()
    }

    /// The newest unsent tick, which standalone interaction packets precede.
    pub fn newest_unsent_sample(&self) -> Option<UnsentSampleView> {
        self.outbox.back().map(UnsentSampleView::from_queued)
    }

    /// The end state of the last completed tick. Vanilla runs build actions before each
    /// simulation tick, so the next tick's build actions observe this state.
    pub fn build_action_state(&self) -> Option<UnsentSampleView> {
        if !self.physics_is_authorized() {
            return None;
        }
        self.tick_ends.get(self.completed_tick())
    }

    /// An anchor restarts the timeline with cleared motion at the last completed tick.
    pub(super) fn anchor_tick_end(&mut self, position: [f32; 3]) {
        self.tick_ends.clear();
        self.tick_ends.record(UnsentSampleView {
            tick: self.completed_tick(),
            position,
            delta: [0.0; 3],
            displacement: [0.0; 3],
            sneaking: false,
        });
    }

    /// A frame-time interaction once every completed tick is on the wire: it reports the
    /// newest admitted tick's position and precedes the next tick's input.
    pub fn between_ticks_sample(&self) -> Option<InteractionSample> {
        if !self.outbox.is_empty() || self.held_release.is_some() {
            return None;
        }
        let (tick, position) = self
            .pending_sends
            .back()
            .map(|pending| {
                (
                    pending.sample.snapshot.tick,
                    pending.sample.snapshot.position,
                )
            })
            .or_else(|| {
                self.sent_history
                    .back()
                    .filter(|sent| sent.session_generation == self.session_generation)
                    .map(|sent| (sent.tick, sent.position))
            })?;
        (tick == self.completed_tick()).then_some(InteractionSample {
            tick: tick.checked_add(1)?,
            position,
        })
    }

    /// Looks up only the exact tick still owned by the unsent movement queue.
    pub fn unsent_sample_at(&self, tick: u64) -> Option<UnsentSampleView> {
        self.outbox
            .iter()
            .find(|sample| sample.snapshot.tick == tick)
            .map(UnsentSampleView::from_queued)
    }

    /// The first eligible unsent tick committed in this render frame.
    pub fn first_unsent_sample_in_frame(&self, recent_ticks: usize) -> Option<UnsentSampleView> {
        if recent_ticks == 0 {
            return None;
        }
        let first = self
            .completed_tick()
            .saturating_sub(recent_ticks as u64 - 1);
        self.outbox
            .iter()
            .find(|sample| sample.snapshot.tick >= first)
            .map(UnsentSampleView::from_queued)
    }

    /// Holds an aim-assisted release until `tick`'s input, which carries the facing it launches
    /// with, is written; later inputs wait behind it and an authority change first drops it.
    pub fn hold_release_after_tick(&mut self, tick: u64, packets: Vec<Packet>) {
        self.held_release = Some(HeldRelease {
            tick,
            packets,
            facing_sent: false,
        });
    }

    /// Whether a held release still waits; later uses must not overtake it.
    pub const fn has_held_release(&self) -> bool {
        self.held_release.is_some()
    }

    /// Sends a held release once its facing tick was written; a full queue keeps the fence.
    pub fn send_held_release(
        &mut self,
        send: impl FnOnce(Vec<Packet>) -> Result<(), crate::BatchSendError>,
    ) {
        let Some(held) = self.held_release.as_ref().filter(|held| held.facing_sent) else {
            return;
        };
        if send(held.packets.clone()) != Err(crate::BatchSendError::Full) {
            self.held_release = None;
        }
    }

    pub(super) fn confirm_held_release_facing(&mut self, tick: u64) {
        if let Some(held) = self.held_release.as_mut().filter(|held| held.tick == tick) {
            held.facing_sent = true;
        }
    }

    /// Action aim overrides actor facing on its unsent tick without changing movement or camera input.
    pub fn override_action_rotation(&mut self, tick: u64, pitch: f32, yaw: f32) -> bool {
        if !pitch.is_finite() || !yaw.is_finite() {
            return false;
        }
        let Some(sample) = self
            .outbox
            .iter_mut()
            .find(|sample| sample.snapshot.tick == tick)
        else {
            return false;
        };
        sample.snapshot.pitch = pitch;
        // Vanilla's aim-assist override leaves head rotation untouched.
        sample.snapshot.yaw = yaw;
        true
    }

    /// Flags an attack press that hit nothing on its unsent or next tick.
    pub fn mark_missed_swing(&mut self, tick: u64) -> bool {
        self.mark_unsent_flag(tick, protocol::PlayerInputFlags::MISSED_SWING)
    }

    /// Flags the unsent or next tick on which the held item's use began.
    pub fn mark_started_using_item(&mut self, tick: u64) -> bool {
        self.mark_unsent_flag(tick, protocol::PlayerInputFlags::START_USING_ITEM)
    }

    fn mark_unsent_flag(&mut self, tick: u64, flag: protocol::PlayerInputFlags) -> bool {
        if let Some(sample) = self
            .outbox
            .iter_mut()
            .find(|sample| sample.snapshot.tick == tick)
        {
            sample.snapshot.flags |= flag;
            return true;
        }
        // A frame-time action between ticks rides the next tick's input, as a jump edge does.
        if tick != self.next_tick || !self.physics_is_authorized() {
            return false;
        }
        let latched = self
            .next_tick_flags
            .filter(|latched| {
                latched.session_generation == self.session_generation && latched.tick == tick
            })
            .map_or(protocol::PlayerInputFlags::NONE, |latched| latched.flags);
        self.next_tick_flags = Some(NextTickFlags {
            session_generation: self.session_generation,
            tick,
            flags: latched | flag,
        });
        true
    }

    /// Flags latched for the tick being built; a stale latch from another session or tick is dropped.
    pub(super) fn take_next_tick_flags(&mut self) -> protocol::PlayerInputFlags {
        self.next_tick_flags
            .take()
            .filter(|latched| {
                latched.session_generation == self.session_generation
                    && latched.tick == self.next_tick
            })
            .map_or(protocol::PlayerInputFlags::NONE, |latched| latched.flags)
    }

    /// Attaches one destroy tick to its exact unsent sample; it is committed from then on.
    pub fn attach_survival_mining(
        &mut self,
        tick: u64,
        payload: crate::survival_mining::SurvivalTickPayload,
    ) -> bool {
        if !self.accepts_block_interactions() {
            return false;
        }
        let Some(sample) = self
            .outbox
            .iter_mut()
            .find(|sample| sample.snapshot.tick == tick && sample.mining.is_none())
        else {
            return false;
        };
        let (interactions, mining_request) = payload.into_interactions(sample.snapshot.position);
        sample.mining = Some(crate::mining::QueuedMiningInteraction {
            interactions,
            mining_request,
        });
        true
    }

    pub const fn interaction_authority_identity(&self) -> (u64, u64) {
        (self.session_generation, self.reanchor_epoch)
    }

    /// Drops every unsent block interaction and revokes admitted ones.
    fn invalidate_mining(&mut self) {
        let next = self.mining_epoch_publisher.borrow().wrapping_add(1);
        self.mining_epoch_publisher.send_replace(next);
        self.outbox
            .iter_mut()
            .for_each(|queued| queued.mining = None);
        self.pending_sends
            .iter_mut()
            .for_each(|pending| pending.sample.mining = None);
    }

    /// Invalidates every transport-owned sample after a position-authority
    /// change and publishes the new epoch atomically with that invalidation.
    pub(super) fn position_authority_changed(&mut self) {
        self.reanchor_epoch = self.reanchor_epoch.wrapping_add(1);
        self.epoch_publisher.send_if_modified(|published| {
            if *published == self.reanchor_epoch {
                false
            } else {
                *published = self.reanchor_epoch;
                true
            }
        });
        // The destroy machine observes the new identity and resets.
        self.invalidate_mining();
        // A release whose facing never reached the socket must not launch without it.
        if self
            .held_release
            .as_ref()
            .is_some_and(|held| !held.facing_sent)
        {
            self.held_release = None;
        }
        for pending in &mut self.pending_sends {
            pending.retry_after_cancellation = false;
        }
        self.sent_history.clear();
        self.refresh_outbox_reconciliation();
    }
}
