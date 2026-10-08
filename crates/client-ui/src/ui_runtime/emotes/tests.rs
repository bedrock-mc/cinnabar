use super::*;

#[test]
fn selecting_equipped_slot_starts_loop_without_changing_other_slots() {
    let mut state = EmoteState::default();
    state.open();
    let emote = state.activate_slot(0, 125).unwrap();
    assert!(!state.is_open());
    let playback = state.playback().unwrap();
    assert_eq!(playback.emote, emote);
    assert_eq!(playback.elapsed(1_125), 1.0);
    assert_eq!(playback.elapsed(0), 0.0);
    assert!(emote.looping());
    assert!(state.take_preferences().is_none());
    state.stop();
    assert!(state.playback().is_none());
}

#[test]
fn change_emotes_moves_a_piece_instead_of_equipping_duplicates_and_persists() {
    let mut state = EmoteState::default();
    let original = state.slots()[0].unwrap();
    state.open();
    state.change_emotes();
    assert!(state.activate_slot(2, 100).is_none());
    assert!(!state.is_equipping());
    assert!(state.is_open());
    assert_eq!(state.slots()[2], Some(original));
    assert!(state.slots()[0].is_none());
    assert!(state.playback().is_none());
    let saved = state.take_preferences().unwrap();
    assert_eq!(saved[2].as_deref(), Some(original.id()));
    let mut restored = EmoteState::default();
    restored.apply_preferences(Some(&saved));
    assert_eq!(restored.slots(), state.slots());
    assert!(state.take_preferences().is_none());
}

#[test]
fn preferences_reject_missing_catalog_entries_and_duplicate_pieces() {
    let id = CustomEmote::ALL[0].id().to_owned();
    let saved = [
        Some(id.clone()),
        Some("missing:fixture".into()),
        Some(id),
        None,
    ];
    let mut state = EmoteState::default();
    state.apply_preferences(Some(&saved));
    assert!(state.slots()[0].is_some());
    assert!(state.slots()[1..].iter().all(Option::is_none));
    state.apply_preferences(Some(&std::array::from_fn(|_| None)));
    state.open();
    assert!(state.activate_slot(0, 0).is_none());
    assert!(
        state.is_open(),
        "empty wheel slots do not play a fabricated emote"
    );
}

#[test]
fn native_navigation_cancel_and_session_reset_preserve_equipped_preferences() {
    let mut state = EmoteState::default();
    state.open();
    for (action, slot) in [([0, -1], 0), ([1, 0], 1), ([0, 1], 2), ([-1, 0], 3)] {
        state.handle_action(UiAction::Navigate(action), 0);
        assert_eq!(state.selected_slot(), Some(slot));
    }
    state.change_emotes();
    state.handle_action(UiAction::Cancel, 0);
    assert!(state.is_open());
    assert!(!state.is_equipping());
    state.handle_action(UiAction::Cancel, 0);
    assert!(!state.is_open());
    state.open();
    state.activate_slot(0, 0);
    let slots = *state.slots();
    state.reset();
    assert!(state.playback().is_none());
    assert!(!state.is_open());
    assert_eq!(state.slots(), &slots);
}
