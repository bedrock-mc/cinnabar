//! Witnesses for the bounded session-owned named-audio resolution slice.

use std::sync::Arc;

use assets::{AudioAlternative, AudioDefinition, RuntimeAudioCatalog, encode_audio_catalog};
use protocol::{AudioEvent, LevelAudioEvent, PlayAudioEvent, StopAudioEvent};

use super::{AudioOutcome, AudioSkipReason, MAX_SESSION_AUDIO_OUTCOMES, SessionAudio};
use crate::runtime::audio::SequencedAudioEvent;

const TEST_MANIFEST_SHA256: [u8; 32] = [0x11; 32];

fn alternative(name: &str, weight: u16) -> AudioAlternative {
    AudioAlternative {
        object_form: true,
        name: name.into(),
        weight,
        volume: None,
        pitch: None,
        is_3d: None,
        stream: None,
        load_on_low_memory: None,
    }
}

fn alternative_with(
    name: &str,
    weight: u16,
    volume: Option<f32>,
    pitch: Option<f32>,
) -> AudioAlternative {
    AudioAlternative {
        object_form: true,
        name: name.into(),
        weight,
        volume,
        pitch,
        is_3d: None,
        stream: None,
        load_on_low_memory: None,
    }
}

fn definition(identifier: &str, alternatives: Vec<AudioAlternative>) -> AudioDefinition {
    definition_with(identifier, alternatives, None, None, None, None)
}

#[allow(clippy::too_many_arguments)]
fn definition_with(
    identifier: &str,
    alternatives: Vec<AudioAlternative>,
    min_distance: Option<f32>,
    max_distance: Option<f32>,
    volume: Option<f32>,
    pitch: Option<f32>,
) -> AudioDefinition {
    AudioDefinition {
        identifier: identifier.into(),
        category: None,
        subtitle: None,
        min_distance,
        max_distance,
        volume,
        pitch,
        use_legacy_max_distance: None,
        alternatives: alternatives.into_boxed_slice(),
    }
}

fn catalog(definitions: &[AudioDefinition]) -> RuntimeAudioCatalog {
    let bytes = encode_audio_catalog(TEST_MANIFEST_SHA256, [0x22; 32], definitions).unwrap();
    RuntimeAudioCatalog::decode(&bytes).unwrap()
}

fn play(sequence: u64, name: &str) -> SequencedAudioEvent {
    play_with(sequence, name, 1.0, 1.0)
}

fn play_with(sequence: u64, name: &str, volume: f32, pitch: f32) -> SequencedAudioEvent {
    SequencedAudioEvent {
        sequence,
        event: AudioEvent::Play(PlayAudioEvent {
            name: Arc::from(name),
            position: [4, -5, 6],
            volume,
            pitch,
            loop_count: 17,
            server_sound_handle: Some(91),
        }),
    }
}

fn stop(sequence: u64) -> SequencedAudioEvent {
    stop_named(sequence, "random.orb", false)
}

fn stop_named(sequence: u64, name: &str, stop_all_sounds: bool) -> SequencedAudioEvent {
    SequencedAudioEvent {
        sequence,
        event: AudioEvent::Stop(StopAudioEvent {
            name: Arc::from(name),
            stop_all_sounds,
            stop_music_legacy: true,
        }),
    }
}

fn level(sequence: u64) -> SequencedAudioEvent {
    SequencedAudioEvent {
        sequence,
        event: AudioEvent::Level(LevelAudioEvent {
            sound_event: Arc::from("step.stone"),
            position: [1.0, 2.0, 3.0],
            data: 0,
            actor_identifier: Arc::from("minecraft:player"),
            is_baby: false,
            is_global: false,
            actor_unique_id: -7,
            fire_at_position: None,
        }),
    }
}

fn resolved(entry: &AudioOutcome) -> &super::ResolvedPlayback {
    match entry {
        AudioOutcome::Play(playback) => playback,
        other => panic!("expected a resolved play outcome, found {other:?}"),
    }
}

#[test]
fn named_play_resolves_a_catalog_hit_with_combined_dynamics_and_wire_passthrough() {
    let catalog = catalog(&[definition_with(
        "random.orb",
        vec![alternative_with("sounds/orb", 1, Some(0.5), None)],
        Some(1.0),
        Some(16.0),
        Some(0.5),
        Some(2.0),
    )]);
    let mut state = SessionAudio::default();

    state.admit(3, 0, [play_with(7, "random.orb", 4.0, 3.0)], Some(&catalog));

    assert_eq!(state.len(), 1);
    let playback = resolved(state.iter().next().expect("single outcome"));
    assert_eq!(playback.sequence, 7);
    assert_eq!(playback.name.as_ref(), "random.orb");
    assert_eq!(playback.alternative_index, 0);
    // definition x packet x alternative gain/pitch combine finitely.
    assert_eq!(playback.gain, 4.0 * 0.5 * 0.5);
    assert_eq!(playback.pitch, 3.0 * 2.0);
    // RAW wire position retained without spatial math.
    assert_eq!(playback.position, [4, -5, 6]);
    assert_eq!(playback.loop_count, 17);
    assert_eq!(playback.min_distance, Some(1.0));
    assert_eq!(playback.max_distance, Some(16.0));
    assert_eq!(playback.server_sound_handle, Some(91));
    assert_eq!(state.unknown_definition_total(), 0);
}

#[test]
fn unknown_names_count_as_unresolved_skips_and_resolve_nothing() {
    let catalog = catalog(&[definition("random.orb", vec![alternative("sounds/orb", 1)])]);
    let mut state = SessionAudio::default();

    state.admit(3, 0, [play(7, "totally.absent")], Some(&catalog));

    assert_eq!(
        state.iter().next().and_then(AudioOutcome::skip_reason),
        Some(AudioSkipReason::UnknownDefinition)
    );
    assert_eq!(state.unknown_definition_total(), 1);
    assert_eq!(state.non_finite_combination_total(), 0);
}

#[test]
fn absent_carrier_counts_every_lookup_as_catalog_unavailable() {
    let mut state = SessionAudio::default();

    state.admit(3, 0, [play(1, "random.orb"), play(2, "note.pling")], None);

    assert_eq!(state.len(), 2);
    for entry in state.iter() {
        assert_eq!(
            entry.skip_reason(),
            Some(AudioSkipReason::CatalogUnavailable)
        );
    }
    assert_eq!(state.catalog_unavailable_total(), 2);
}

#[test]
fn zero_alternative_definitions_count_as_empty_alternative_skips() {
    let catalog = catalog(&[definition("silent.block", Vec::new())]);
    let mut state = SessionAudio::default();

    state.admit(3, 0, [play(9, "silent.block")], Some(&catalog));

    assert_eq!(
        state.iter().next().and_then(AudioOutcome::skip_reason),
        Some(AudioSkipReason::EmptyAlternatives)
    );
    assert_eq!(state.empty_alternatives_total(), 1);
}

#[test]
fn selection_is_deterministic_for_fixed_inputs() {
    let catalog = catalog(&[definition(
        "random.orb",
        vec![alternative("sounds/a", 1), alternative("sounds/b", 1)],
    )]);

    let first = resolve_single_index(&catalog, 42, "random.orb");
    let second = resolve_single_index(&catalog, 42, "random.orb");
    assert_eq!(first, second);
}

#[test]
fn weighted_selection_follows_the_declared_alternative_weights() {
    let catalog = catalog(&[definition(
        "random.orb",
        vec![
            alternative("sounds/rare", 1),
            alternative("sounds/common", 65_535),
        ],
    )]);

    let samples = 200_usize;
    let common_hits = (0_u64..u64::try_from(samples).unwrap())
        .map(|sequence| resolve_single_name(&catalog, sequence, "random.orb"))
        .filter(|name| name == "sounds/common")
        .count();
    // Weight 65535/65536 selects the heavy route almost always; any material
    // deviation would mean the seed is ignoring declared weights.
    assert!(
        common_hits * 100 >= samples * 95,
        "weighted selection drifted: {common_hits}/{samples}"
    );
}

#[test]
fn balanced_weights_select_both_alternatives_across_a_sequence_range() {
    let catalog = catalog(&[definition(
        "random.orb",
        vec![alternative("sounds/a", 1), alternative("sounds/b", 1)],
    )]);

    let seen: std::collections::BTreeSet<usize> = (0..200_u64)
        .map(|sequence| resolve_single_index(&catalog, sequence, "random.orb"))
        .collect();
    assert_eq!(seen.len(), 2, "balanced weights must reach both routes");
}

fn resolve_single_index(catalog: &RuntimeAudioCatalog, sequence: u64, name: &str) -> usize {
    let mut state = SessionAudio::default();
    state.admit(3, 0, [play(sequence, name)], Some(catalog));
    resolved(state.iter().next().expect("one outcome")).alternative_index
}

/// Resolves the selected alternative's route name so weight witnesses do not
/// depend on the encoder's canonical alternative ordering.
fn resolve_single_name(catalog: &RuntimeAudioCatalog, sequence: u64, name: &str) -> String {
    let index = resolve_single_index(catalog, sequence, name);
    catalog
        .lookup(name)
        .and_then(|definition| definition.alternatives.get(index))
        .map(|alternative| alternative.name.to_string())
        .expect("resolved index stays inside the definition")
}

#[test]
fn non_finite_combinations_count_without_making_a_decision() {
    let catalog = catalog(&[definition_with(
        "extreme.sound",
        vec![alternative_with("sounds/extreme", 1, Some(2.0), None)],
        None,
        None,
        None,
        None,
    )]);
    let mut state = SessionAudio::default();

    // Packet dynamics are finite but overflow f32 once combined with the
    // definition scale; the slice must count the skip instead of resolving.
    state.admit(
        3,
        0,
        [play_with(11, "extreme.sound", 3.0e38, 1.0)],
        Some(&catalog),
    );

    assert_eq!(
        state.iter().next().and_then(AudioOutcome::skip_reason),
        Some(AudioSkipReason::NonFiniteCombination)
    );
    assert_eq!(state.non_finite_combination_total(), 1);
}

#[test]
fn named_stops_resolve_without_the_catalog() {
    let mut state = SessionAudio::default();

    state.admit(3, 0, [stop_named(5, "music.game", false)], None);

    assert_eq!(state.len(), 1);
    match state.iter().next().expect("stop outcome") {
        AudioOutcome::Stop {
            sequence,
            name,
            stop_all_sounds,
            stop_music_legacy,
        } => {
            assert_eq!(*sequence, 5);
            assert_eq!(name.as_ref(), "music.game");
            assert!(!stop_all_sounds);
            assert!(*stop_music_legacy);
        }
        other => panic!("expected a stop outcome, found {other:?}"),
    }
    assert_eq!(state.catalog_unavailable_total(), 0);
}

#[test]
fn stop_all_sounds_is_honored_regardless_of_the_name() {
    let mut state = SessionAudio::default();

    state.admit(3, 0, [stop_named(6, "", true)], None);

    match state.iter().next().expect("stop outcome") {
        AudioOutcome::Stop {
            name,
            stop_all_sounds,
            ..
        } => {
            assert!(stop_all_sounds);
            assert_eq!(name.as_ref(), "");
        }
        other => panic!("expected a stop outcome, found {other:?}"),
    }
}

#[test]
fn level_sound_events_are_transport_only_counts_without_outcomes() {
    let mut state = SessionAudio::default();

    state.admit(3, 0, [level(1), play(2, "random.orb"), level(3)], None);

    assert_eq!(state.level_transport_only_total(), 2);
    assert_eq!(state.len(), 1);
    let sequences: Vec<u64> = state
        .iter()
        .filter_map(|outcome| outcome.sequence())
        .collect();
    assert_eq!(
        sequences,
        vec![2],
        "transport-only records never occupy the queue"
    );
}

#[test]
fn admission_preserves_fifo_order_across_interleaved_families() {
    let catalog = catalog(&[definition("random.orb", vec![alternative("sounds/orb", 1)])]);
    let mut state = SessionAudio::default();

    state.admit(
        3,
        0,
        [
            stop(1),
            play(2, "random.orb"),
            play(3, "absent.sound"),
            level(4),
            stop(5),
        ],
        Some(&catalog),
    );

    let sequences: Vec<u64> = state
        .iter()
        .filter_map(|outcome| outcome.sequence())
        .collect();
    assert_eq!(sequences, vec![1, 2, 3, 5]);
}

#[test]
fn overflow_drops_the_oldest_entry_with_accounting_and_recovers() {
    let mut state = SessionAudio::default();
    let flood: Vec<SequencedAudioEvent> = (1..=(MAX_SESSION_AUDIO_OUTCOMES as u64 + 4))
        .map(stop)
        .collect();
    state.admit(3, 0, flood, None);

    assert_eq!(state.len(), MAX_SESSION_AUDIO_OUTCOMES);
    let oldest = state.iter().next().expect("oldest");
    assert_eq!(oldest.sequence(), Some(5));
    assert_eq!(state.dropped_oldest_total(), 4);

    // Recovery: capacity freed by the drops admits newer outcomes again.
    state.admit(3, 0, [stop(10_001)], None);
    assert_eq!(state.len(), MAX_SESSION_AUDIO_OUTCOMES);
    assert_eq!(
        state.iter().last().expect("newest").sequence(),
        Some(10_001)
    );
    assert_eq!(state.dropped_oldest_total(), 5);
    assert_eq!(
        state.admitted_total(),
        MAX_SESSION_AUDIO_OUTCOMES as u64 + 5
    );
}

#[test]
fn session_replacement_clears_retained_outcomes_and_counts_one_reset() {
    let mut state = SessionAudio::default();
    state.admit(3, 0, [stop(1)], None);

    state.admit(4, 0, [stop(2)], None);

    assert_eq!(state.len(), 1);
    assert_eq!(
        state.iter().next().and_then(AudioOutcome::sequence),
        Some(2)
    );
    assert_eq!(state.resets(), 1);
    // Lifetime accounting survives the reset so drop evidence is never lost.
    assert_eq!(state.admitted_total(), 2);
    assert_eq!(state.dropped_oldest_total(), 0);
}

#[test]
fn dimension_change_clears_retained_outcomes() {
    let mut state = SessionAudio::default();
    state.admit(3, 0, [stop(1), stop(2)], None);

    state.admit(3, 1, [stop(3)], None);

    assert_eq!(state.len(), 1);
    assert_eq!(state.resets(), 1);
}

#[test]
fn unchanged_identity_keeps_history_and_counts_no_reset() {
    let mut state = SessionAudio::default();
    state.admit(3, 0, [stop(1)], None);
    state.admit(3, 0, [stop(2)], None);

    assert_eq!(state.len(), 2);
    assert_eq!(state.resets(), 0);
}

#[test]
fn idle_admission_under_changed_identity_clears_retained_outcomes() {
    let mut state = SessionAudio::default();
    state.admit(3, 0, [stop(1), stop(2)], None);

    // Mirrors the accepted camera window: identity is refreshed on every
    // drain, so an idle drain after a dimension switch still clears.
    state.admit(3, 1, [], None);

    assert!(state.is_empty());
    assert_eq!(state.resets(), 1);
}
