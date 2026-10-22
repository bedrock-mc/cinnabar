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
use simulation_clock::identity::{PositionAuthorityGeneration, SessionGeneration};

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
    QueuedPhysicsSample, SentPhysicsSample,
};

/// A release batch fenced behind the input tick that carries its facing.
#[derive(Debug, Clone)]
pub(super) struct HeldRelease {
    tick: u64,
    packets: Vec<Packet>,
    facing_sent: bool,
    use_modifier: Option<f64>,
    riptide: Option<super::physics::RiptideRequest>,
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
        |identity, packet, _guard, _sampled_at| send(identity, packet),
    )
}

/// Hands each tick to transport with its original sampling time, including retries.
pub fn flush_player_auth_inputs_guarded<E>(
    ticker: &mut MovementTicker,
    budget: usize,
    evidence_context: Option<PhysicsTickEvidenceContext>,
    mut send: impl FnMut(
        PhysicsSendIdentity,
        Packet,
        Option<InteractionPacketGuard>,
        Option<std::time::Duration>,
    ) -> Result<(), E>,
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
                let mut guard = InteractionPacketGuard::new(
                    *publisher.borrow(),
                    publisher.subscribe(),
                    movement_packet,
                );
                if let Some(action) = sample
                    .mining
                    .as_ref()
                    .and_then(|mining| mining.creative_destroy)
                {
                    guard = guard.with_preceding_packet(protocol::creative_destroy_block_packet(
                        action.local_runtime_id,
                        action.position,
                        action.face,
                    ));
                }
                Ok::<_, PlayerAuthInputError>(guard)
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
        let sampled_at = sample.sampled_at;
        ticker.note_command_admitted(
            identity,
            sample,
            evidence_context.expect("nonempty outbox requires staged evidence context"),
        );
        if let Err(error) = send(identity, packet, interaction_guard, sampled_at) {
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
    /// Retains the immutable state of a movement input written to the socket.
    fn confirm_sent(&mut self, sample: &QueuedPhysicsSample) {
        if self.sent_history.len() == OUTBOX_CAPACITY {
            self.sent_history.pop_front();
        }
        self.sent_history.push_back(SentPhysicsSample {
            session_generation: sample.session_generation,
            tick: sample.snapshot.tick,
            position: sample.snapshot.position,
            world_identity: sample.world_identity.clone(),
        });
    }

    /// Commits the oldest matching socket receipt and retires its release launch.
    pub fn acknowledge_physics_send(&mut self, identity: PhysicsSendIdentity) -> bool {
        if self
            .pending_sends
            .front()
            .is_none_or(|pending| pending.identity != identity)
        {
            return false;
        }
        if self.tick_evidence.len() == OUTBOX_CAPACITY {
            self.pending_sends.pop_front();
            self.fail_physics_authority(&PhysicsAuthorityFault::OutboxOverflow);
            return false;
        }
        let pending = self
            .pending_sends
            .pop_front()
            .expect("matching pending socket acknowledgement was checked");
        if self.physics_is_authorized()
            && identity.session_generation.get() == self.session_generation
            && identity.reanchor_epoch == self.reanchor_epoch
        {
            self.confirm_sent(&pending.sample);
            self.confirm_held_release_facing(identity.tick.get());
        }
        if identity.session_generation.get() == self.session_generation {
            self.confirm_riptide_input_written(identity.tick.get());
        }
        self.sent_physics_packet_count = self.sent_physics_packet_count.saturating_add(1);
        self.tick_evidence.push_back(pending.evidence);
        self.refresh_outbox_reconciliation();
        true
    }

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

    /// A frame-time interaction once every completed tick has entered the outbound FIFO.
    /// It reports the newest admitted tick's position and precedes the next tick's input.
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

    /// Whether an interaction precedes the first unsent input, without needing pose history.
    pub fn is_first_unsent_tick(&self, tick: u64) -> bool {
        self.outbox
            .front()
            .is_some_and(|sample| sample.snapshot.tick == tick)
    }

    /// Pairs an attack with the state before the first input it precedes.
    /// Older unsent inputs must drain before an attack can use a later state.
    pub fn interaction_before_tick(&self, tick: u64) -> Option<InteractionSample> {
        if !self.is_first_unsent_tick(tick) {
            return None;
        }
        let previous = self.tick_ends.get(tick.checked_sub(1)?)?;
        Some(InteractionSample {
            tick,
            position: previous.position,
        })
    }

    /// Holds a release until its last preceding input is written, preserving facing and launch
    /// order. Later inputs wait behind it and an authority change first drops it.
    pub fn hold_release_after_tick(&mut self, tick: u64, packets: Vec<Packet>) {
        self.held_release = Some(HeldRelease {
            tick,
            packets,
            facing_sent: false,
            use_modifier: None,
            riptide: None,
        });
    }

    /// Binds a predicted launch to its held release, so discarding the fence cancels both.
    pub fn request_riptide(
        &mut self,
        physics: &mut super::LocalPhysicsController,
        level: u8,
        rotation: [f32; 2],
    ) {
        let tick = self
            .held_release
            .as_ref()
            .map_or(self.completed_tick(), |held| held.tick);
        if let Some(request) = physics.request_riptide_after_tick(level, rotation, tick) {
            if let Some(held) = self.held_release.as_mut() {
                held.riptide = Some(request);
            } else {
                self.admitted_riptide = Some(request);
            }
        }
    }

    /// Returns the launch boundary that a correction will discard with its unsent facing.
    pub(super) fn unconfirmed_riptide_release_tick(&self) -> Option<u64> {
        self.held_release
            .as_ref()
            .filter(|held| held.riptide.is_some() && !held.facing_sent)
            .map(|held| held.tick)
    }

    /// Preserves a written release's request, or an already-admitted pending launch, on snap.
    pub(super) fn riptide_request_after_snap(
        &self,
        physics: &super::LocalPhysicsController,
    ) -> Option<super::physics::RiptideRequest> {
        self.admitted_riptide.or_else(|| {
            match self
                .held_release
                .as_ref()
                .filter(|held| held.riptide.is_some())
            {
                Some(held) if held.facing_sent => held.riptide,
                Some(_) => None,
                None => physics.pending_riptide_request().filter(|request| {
                    self.written_riptide_tick
                        .is_none_or(|tick| tick <= request.after_tick)
                }),
            }
        })
    }

    /// Captures movement use state separately from the release's frozen charge decision.
    pub fn retain_release_use_modifier(&mut self, modifier: Option<f64>) {
        if let Some(held) = self.held_release.as_mut() {
            held.use_modifier = modifier;
        }
    }

    /// Keeps slowdown through the facing tick, independently of later transport pressure.
    pub fn release_use_modifier(&self) -> Option<(u64, f64)> {
        let held = self.held_release.as_ref()?;
        Some((held.tick, held.use_modifier?))
    }

    /// Retires only the admitted release preceding this written movement input.
    pub(super) fn confirm_riptide_input_written(&mut self, tick: u64) {
        if self
            .admitted_riptide
            .is_some_and(|request| tick > request.after_tick)
        {
            self.admitted_riptide = None;
            self.written_riptide_tick = Some(tick);
        }
    }

    /// Resolves a late socket receipt before a restored launch can run again.
    pub(super) fn settle_riptide_input(&self, physics: &mut super::LocalPhysicsController) {
        if let Some(tick) = self.written_riptide_tick {
            physics.retire_written_riptide_request(tick);
        }
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
        match send(held.packets.clone()) {
            Ok(()) => {
                // FIFO facing receipts ensure an earlier launch is written before a later
                // release can be admitted, so at most one admitted launch needs retention.
                if let Some(request) = held.riptide {
                    self.admitted_riptide = Some(request);
                }
                self.held_release = None;
            }
            Err(crate::BatchSendError::Full) => {}
            Err(crate::BatchSendError::Closed) => self.held_release = None,
        }
    }

    pub(super) fn confirm_held_release_facing(&mut self, tick: u64) {
        if let Some(held) = self.held_release.as_mut().filter(|held| held.tick == tick) {
            held.facing_sent = true;
        }
    }

    /// Overrides actor facing on an unsent tick, or reserves the next tick after inputs drain.
    /// Movement, head facing and camera input keep their sampled values.
    pub fn override_action_rotation(&mut self, tick: u64, pitch: f32, yaw: f32) -> bool {
        if !pitch.is_finite() || !yaw.is_finite() {
            return false;
        }
        let Some(sample) = self
            .outbox
            .iter_mut()
            .find(|sample| sample.snapshot.tick == tick)
        else {
            if self
                .between_ticks_sample()
                .is_some_and(|sample| sample.tick == tick)
                && self.physics_is_authorized()
            {
                self.next_tick_rotation = Some((pitch, yaw));
                return true;
            }
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
        self.next_tick_flags |= flag;
        true
    }

    /// Action flags for the tick being built; corrections keep them, authority changes drop them.
    pub(super) fn take_next_tick_flags(&mut self) -> protocol::PlayerInputFlags {
        std::mem::replace(&mut self.next_tick_flags, protocol::PlayerInputFlags::NONE)
    }

    /// Whether committed destroy maintenance already owns this unsent input.
    pub(crate) fn carries_survival_mining(&self, tick: u64) -> bool {
        self.outbox
            .iter()
            .any(|sample| sample.snapshot.tick == tick && sample.mining.is_some())
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
        let creative_destroy = payload.creative_destroy;
        let (interactions, mining_request) = payload.into_interactions(sample.snapshot.position);
        sample.mining = Some(crate::mining::QueuedMiningInteraction {
            creative_destroy,
            interactions,
            mining_request,
        });
        true
    }

    /// Identifies the interactions that remain valid: a session change or a reanchor that
    /// revokes queued block actions starts a new identity, while a replayed correction keeps it.
    pub const fn interaction_authority_identity(
        &self,
    ) -> (SessionGeneration, PositionAuthorityGeneration) {
        (
            SessionGeneration::new(self.session_generation),
            self.interaction_epoch,
        )
    }

    /// Identifies geometry such as eye rays and picks: every reanchor, including a replayed
    /// correction that moves the player, retires geometry captured before it.
    pub const fn position_authority_identity(
        &self,
    ) -> (SessionGeneration, PositionAuthorityGeneration) {
        (
            SessionGeneration::new(self.session_generation),
            self.reanchor_epoch,
        )
    }

    /// Whether interactions only wait for inputs written before a position-authority change.
    pub fn settling_position_authority(&self) -> bool {
        self.physics_is_authorized()
            && !self.terminal_drain
            && self.has_unresolved_position_authority_change()
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

    /// A correction keeps the player's pending action flags. A snap revokes in-flight
    /// interactions; a replay keeps them unless it drops a tick that carried or awaits one.
    pub(super) fn correction_authority_changed(&mut self, revoke_interactions: bool) {
        let flags = self.take_next_tick_flags();
        let discarded_facing = self
            .held_release
            .as_ref()
            .is_some_and(|held| !held.facing_sent && held.tick == self.next_tick);
        let rotation = self.next_tick_rotation.take().filter(|_| !discarded_facing);
        self.reanchor_position_authority(revoke_interactions);
        self.next_tick_flags = flags;
        self.next_tick_rotation = rotation;
    }

    /// Invalidates every transport-owned sample and queued interaction after a
    /// position-authority change.
    pub(super) fn position_authority_changed(&mut self) {
        self.reanchor_position_authority(true);
    }

    /// Publishes a new transport epoch atomically with sample invalidation. Revoking
    /// interactions also starts a new interaction identity, which resets their owners.
    fn reanchor_position_authority(&mut self, revoke_interactions: bool) {
        self.next_tick_flags = protocol::PlayerInputFlags::NONE;
        self.next_tick_rotation = None;
        self.reanchor_epoch = self.reanchor_epoch.next();
        self.epoch_publisher.send_if_modified(|published| {
            if *published == self.reanchor_epoch {
                false
            } else {
                *published = self.reanchor_epoch;
                true
            }
        });
        if revoke_interactions {
            // The destroy machine observes the new identity and resets.
            self.interaction_epoch = self.interaction_epoch.next();
            self.invalidate_mining();
        }
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

impl MovementTicker {
    /// Checks whether a press can use the first pending or next movement tick.
    pub fn interaction_tick_available(&self, tick: u64) -> bool {
        self.is_first_unsent_tick(tick)
            || self.between_ticks_sample().is_some_and(|s| s.tick == tick)
    }
}
