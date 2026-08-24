//! Bounded session-owned named-audio resolution state.
//!
//! This slice resolves committed named `PlaySound`/`StopSound` traffic into
//! retained outcomes only. There is deliberately no audible output, no audio
//! backend or mixer dependency, no listener math, no category settings, and
//! no resource-pack routing here; a future playback consumer reads the
//! retained outcomes and owns all of those semantics.
//!
//! `LevelSoundEvent` records are transport-only in this slice: numeric/string
//! level routing is unverified against vanilla, so they are counted without
//! producing outcomes. They stay high-frequency on real servers, so counting
//! instead of retaining also keeps this bounded queue from being flooded by
//! records this slice must not resolve anyway.

use std::collections::VecDeque;
use std::sync::Arc;

use assets::RuntimeAudioCatalog;
use bevy::prelude::{MessageReader, Res, ResMut, Resource};

use crate::runtime::audio::SequencedAudioEvent;

/// Maximum retained audio outcomes.
///
/// Named audio commands are rare relative to level events, so this ceiling
/// only guards against a hostile or broken server flooding the queue between
/// render frames.
pub const MAX_SESSION_AUDIO_OUTCOMES: usize = 256;

/// Startup-bound optional sound-definition catalog.
///
/// Per the VPA-017 owner decision this binds optionally: absence falls back to
/// `None` with a one-time startup notice (every lookup counts as an unresolved
/// skip), while a present-but-malformed, oversized, or stale-provenance
/// carrier fails startup closed in [`crate::asset_startup`].
#[derive(Clone, Debug, Default, Resource)]
pub struct SessionAudioCatalog(pub Option<Arc<RuntimeAudioCatalog>>);

/// Why a play event resolved to nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioSkipReason {
    /// No catalog was bound at startup; lookups cannot resolve.
    CatalogUnavailable,
    /// The named definition is absent from the pinned catalog.
    UnknownDefinition,
    /// The definition exists but carries no alternative routes.
    EmptyAlternatives,
    /// definition x packet x alternative dynamics overflowed finite `f32`.
    NonFiniteCombination,
}

/// One resolved named-play decision, ready for a future playback consumer.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedPlayback {
    pub sequence: u64,
    pub name: Arc<str>,
    /// Index into the definition's canonical alternative list.
    pub alternative_index: usize,
    /// Finite-checked combined gain (definition x packet x alternative).
    pub gain: f32,
    /// Finite-checked combined pitch (definition x packet x alternative).
    pub pitch: f32,
    /// RAW wire position retained verbatim without spatial math.
    pub position: [i32; 3],
    /// Raw packet loop count; interpretation belongs to the playback consumer.
    pub loop_count: i32,
    pub min_distance: Option<f32>,
    pub max_distance: Option<f32>,
    /// Preserved untouched through resolution for a future id-348 controls family.
    pub server_sound_handle: Option<u64>,
}

/// One admitted audio outcome.
#[derive(Debug, Clone, PartialEq)]
pub enum AudioOutcome {
    Play(ResolvedPlayback),
    Stop {
        sequence: u64,
        name: Arc<str>,
        stop_all_sounds: bool,
        stop_music_legacy: bool,
    },
    Skipped {
        sequence: u64,
        reason: AudioSkipReason,
    },
}

impl AudioOutcome {
    /// Sequence number carried by any outcome variant.
    #[must_use]
    pub const fn sequence(&self) -> Option<u64> {
        match self {
            Self::Play(playback) => Some(playback.sequence),
            Self::Stop { sequence, .. } | Self::Skipped { sequence, .. } => Some(*sequence),
        }
    }

    /// Skip reason when this outcome resolved nothing.
    #[must_use]
    pub const fn skip_reason(&self) -> Option<AudioSkipReason> {
        match self {
            Self::Skipped { reason, .. } => Some(*reason),
            _ => None,
        }
    }
}

/// Bounded ordered admission state for session-owned audio outcomes.
///
/// A session-generation change or a dimension change clears the retained
/// outcomes whenever that identity mismatch is next observed, including idle
/// drains with no incoming audio traffic; lifetime counters survive so
/// overflow evidence is never lost.
///
/// Bounded accepted window (mirroring the reviewed server-camera semantics):
/// identity is sampled once per drain from the caller's current world state
/// rather than per packet. Audio events written into the same poll batch as a
/// dimension switch may therefore be admitted under the new identity instead
/// of being cleared as prior-dimension state; correcting that requires
/// cross-packet reordering this surface deliberately does not perform, and the
/// window closes at the next drain after the switch.
#[derive(Debug, Default, Resource)]
pub struct SessionAudio {
    entries: VecDeque<AudioOutcome>,
    admitted_total: u64,
    dropped_oldest_total: u64,
    resets: u64,
    unknown_definition_total: u64,
    non_finite_combination_total: u64,
    empty_alternatives_total: u64,
    catalog_unavailable_total: u64,
    level_transport_only_total: u64,
    identity: Option<(u64, i32)>,
}

impl SessionAudio {
    fn refresh_identity(&mut self, session_generation: u64, dimension: i32) {
        if let Some(previous) = self.identity
            && previous != (session_generation, dimension)
        {
            self.entries.clear();
            self.resets = self.resets.saturating_add(1);
        }
        self.identity = Some((session_generation, dimension));
    }

    fn skip_total_mut(&mut self, reason: AudioSkipReason) -> &mut u64 {
        match reason {
            AudioSkipReason::CatalogUnavailable => &mut self.catalog_unavailable_total,
            AudioSkipReason::UnknownDefinition => &mut self.unknown_definition_total,
            AudioSkipReason::EmptyAlternatives => &mut self.empty_alternatives_total,
            AudioSkipReason::NonFiniteCombination => &mut self.non_finite_combination_total,
        }
    }

    /// Admits sequenced audio events under one `(session, dimension)` identity.
    ///
    /// An identity change clears retained outcomes before admitting the new
    /// events; the first admission binds the identity without counting a reset.
    /// Overflow drops the oldest outcome instead of rejecting newer,
    /// authoritative server traffic. Stops resolve without the catalog;
    /// level events count as transport-only and never occupy the queue.
    pub(crate) fn admit(
        &mut self,
        session_generation: u64,
        dimension: i32,
        events: impl IntoIterator<Item = SequencedAudioEvent>,
        catalog: Option<&RuntimeAudioCatalog>,
    ) {
        self.refresh_identity(session_generation, dimension);
        let mut admitted = 0_u64;
        for SequencedAudioEvent { sequence, event } in events {
            let outcome = match event {
                protocol::AudioEvent::Play(play) => self.resolve_play(catalog, sequence, &play),
                protocol::AudioEvent::Stop(stop) => AudioOutcome::Stop {
                    sequence,
                    name: Arc::clone(&stop.name),
                    stop_all_sounds: stop.stop_all_sounds,
                    stop_music_legacy: stop.stop_music_legacy,
                },
                // Transport-only by contract: retain nothing, count everything.
                protocol::AudioEvent::Level(_) => {
                    self.level_transport_only_total =
                        self.level_transport_only_total.saturating_add(1);
                    continue;
                }
            };
            if let AudioOutcome::Skipped { reason, .. } = &outcome {
                let total = self.skip_total_mut(*reason);
                *total = total.saturating_add(1);
            }
            while self.entries.len() >= MAX_SESSION_AUDIO_OUTCOMES {
                self.entries.pop_front();
                self.dropped_oldest_total = self.dropped_oldest_total.saturating_add(1);
            }
            self.entries.push_back(outcome);
            admitted = admitted.saturating_add(1);
        }
        self.admitted_total = self.admitted_total.saturating_add(admitted);
    }

    fn resolve_play(
        &self,
        catalog: Option<&RuntimeAudioCatalog>,
        sequence: u64,
        play: &protocol::PlayAudioEvent,
    ) -> AudioOutcome {
        let skip = |reason| AudioOutcome::Skipped { sequence, reason };
        let Some(catalog) = catalog else {
            return skip(AudioSkipReason::CatalogUnavailable);
        };
        let Some(definition) = catalog.lookup(&play.name) else {
            return skip(AudioSkipReason::UnknownDefinition);
        };
        if definition.alternatives.is_empty() {
            return skip(AudioSkipReason::EmptyAlternatives);
        }
        let alternative_index =
            select_alternative(sequence, play.name.as_bytes(), &definition.alternatives);
        let alternative = &definition.alternatives[alternative_index];
        // Packet dynamics arrive finite from the protocol boundary and catalog
        // values are finite-checked at decode, but products can still overflow
        // f32 range, so each combination is re-checked here before deciding.
        let Some(gain) = combine(play.volume, definition.volume, alternative.volume) else {
            return skip(AudioSkipReason::NonFiniteCombination);
        };
        let Some(pitch) = combine(play.pitch, definition.pitch, alternative.pitch) else {
            return skip(AudioSkipReason::NonFiniteCombination);
        };
        AudioOutcome::Play(ResolvedPlayback {
            sequence,
            name: Arc::clone(&play.name),
            alternative_index,
            gain,
            pitch,
            position: play.position,
            loop_count: play.loop_count,
            min_distance: definition.min_distance,
            max_distance: definition.max_distance,
            server_sound_handle: play.server_sound_handle,
        })
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    #[must_use]
    pub fn iter(&self) -> std::collections::vec_deque::Iter<'_, AudioOutcome> {
        self.entries.iter()
    }

    /// Cumulative retained outcome count across resets.
    #[must_use]
    pub const fn admitted_total(&self) -> u64 {
        self.admitted_total
    }

    /// Cumulative oldest-entry drops from capacity overflow.
    #[must_use]
    pub const fn dropped_oldest_total(&self) -> u64 {
        self.dropped_oldest_total
    }

    /// Cumulative session/dimension identity changes observed at drain time.
    #[must_use]
    pub const fn resets(&self) -> u64 {
        self.resets
    }

    #[must_use]
    pub const fn unknown_definition_total(&self) -> u64 {
        self.unknown_definition_total
    }

    #[must_use]
    pub const fn non_finite_combination_total(&self) -> u64 {
        self.non_finite_combination_total
    }

    #[must_use]
    pub const fn empty_alternatives_total(&self) -> u64 {
        self.empty_alternatives_total
    }

    #[must_use]
    pub const fn catalog_unavailable_total(&self) -> u64 {
        self.catalog_unavailable_total
    }

    /// Level sound events seen and deliberately left transport-only.
    #[must_use]
    pub const fn level_transport_only_total(&self) -> u64 {
        self.level_transport_only_total
    }

    /// Drops every retained outcome without touching lifetime counters.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

/// PROVISIONAL deterministic weighted alternative selection.
///
/// Vanilla Bedrock selects among weighted alternatives at playback time with
/// engine randomness that has not been observed natively for this contract.
/// Until version-matched native evidence exists, selection is a pure function
/// of `(sequence, name bytes)` through FNV-1a so replays and tests stay stable;
/// it makes no claim about matching vanilla distributions.
const FNV1A_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV1A_PRIME: u64 = 0x0000_0100_0000_01b3;

fn selection_seed(sequence: u64, name: &[u8]) -> u64 {
    let mut seed = FNV1A_OFFSET_BASIS;
    for byte in sequence.to_le_bytes() {
        seed = (seed ^ u64::from(byte)).wrapping_mul(FNV1A_PRIME);
    }
    for byte in name {
        seed = (seed ^ u64::from(*byte)).wrapping_mul(FNV1A_PRIME);
    }
    seed
}

fn select_alternative(
    sequence: u64,
    name: &[u8],
    alternatives: &[assets::AudioAlternative],
) -> usize {
    // Weights are strictly positive and bounded (<= 64 alternatives x u16), so
    // the running total always fits u32 comfortably.
    let total_weight: u32 = alternatives.iter().map(|alt| u32::from(alt.weight)).sum();
    if total_weight == 0 {
        return 0;
    }
    let point = selection_seed(sequence, name) % u64::from(total_weight);
    let mut cumulative: u32 = 0;
    for (index, alternative) in alternatives.iter().enumerate() {
        cumulative = cumulative.saturating_add(u32::from(alternative.weight));
        if point < u64::from(cumulative) {
            return index;
        }
    }
    alternatives.len().saturating_sub(1)
}

fn combine(packet: f32, definition: Option<f32>, alternative: Option<f32>) -> Option<f32> {
    let mut value = packet;
    if let Some(scale) = definition {
        value *= scale;
    }
    if let Some(scale) = alternative {
        value *= scale;
    }
    value.is_finite().then_some(value)
}

/// Reads committed sequenced audio messages into the bounded session state,
/// refreshing the stored identity on every call so an identity change clears
/// retained outcomes even when no audio batch arrived.
pub(crate) fn drain_sequenced_audio_into_session(
    mut messages: MessageReader<SequencedAudioEvent>,
    clock: Res<crate::environment::WorldClock>,
    client_world: Res<crate::runtime::world::ClientWorld>,
    catalog: Res<SessionAudioCatalog>,
    mut session: ResMut<SessionAudio>,
) {
    let Some(stream) = client_world.stream.as_ref() else {
        // Mirrors reconcile_world_stream_before_physics: without a live world
        // stream no writer ran this frame, so identity stays unsampled.
        return;
    };
    let events: Vec<_> = messages.read().cloned().collect();
    session.admit(
        clock.session_generation(),
        stream.current_dimension(),
        events,
        catalog.0.as_deref(),
    );
}

#[cfg(test)]
#[path = "session_audio_tests.rs"]
mod session_audio_tests;
