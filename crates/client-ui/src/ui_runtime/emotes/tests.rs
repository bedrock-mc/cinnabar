use super::*;

#[test]
fn change_emotes_lists_owned_catalog_clips_and_equips_the_chosen_entry() {
    let _catalog = cosmetics::test_support::catalog_lock();
    let free = cosmetics::test_support::emote("fixture:wheel", "Wheel fixture", true);
    let paid = cosmetics::test_support::emote("fixture:unowned", "Unowned fixture", false);
    cosmetics::emotes::activate(&[free, paid]);
    let mut state = EmoteState::default();
    state.open();
    state.change_emotes();
    let choices = state.display_slots();
    assert!(choices.contains(&Some(free)));
    assert!(!choices.contains(&Some(paid)));
    let index = choices
        .iter()
        .position(|value| *value == Some(free))
        .unwrap();
    state.activate_slot(index, 0);
    assert!(state.is_equipping());
    state.activate_slot(3, 0);
    let saved = state.take_preferences().unwrap();
    assert_eq!(saved[3].as_deref(), Some(free.descriptor().id.as_str()));
    state.open();
    assert_eq!(state.activate_slot(3, 250), Some(free));
    assert_eq!(state.playback().unwrap().elapsed(750), 0.5);
}

#[test]
fn selecting_equipped_slot_starts_loop_without_changing_other_slots() {
    let _catalog = cosmetics::test_support::playback_catalog();
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
    let _catalog = cosmetics::test_support::playback_catalog();
    let mut state = EmoteState::default();
    let original = state.slots()[0].unwrap();
    state.open();
    state.change_emotes();
    state.activate_slot(0, 100);
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
    let _catalog = cosmetics::test_support::playback_catalog();
    let id = cosmetics::test_support::playback_emote().id().to_owned();
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
    let _catalog = cosmetics::test_support::playback_catalog();
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

#[test]
fn restarting_at_the_same_clock_value_has_a_new_broadcast_identity() {
    let _catalog = cosmetics::test_support::playback_catalog();
    let mut state = EmoteState::default();
    state.open();
    state.activate_slot(0, 100);
    let first = state.playback().unwrap();
    state.stop();
    state.open();
    state.activate_slot(0, 100);
    let second = state.playback().unwrap();
    assert_eq!(first.start_millis, second.start_millis);
    assert_ne!(first.revision, second.revision);
}

#[test]
fn catalog_pages_cover_owned_clips_and_retire_removed_choices_and_playback() {
    let _catalog = cosmetics::test_support::catalog_lock();
    let clips: Vec<_> = (0..9)
        .map(|index| {
            cosmetics::test_support::emote(&format!("fixture:page_{index}"), "Page fixture", true)
        })
        .collect();
    cosmetics::emotes::activate(&clips);
    let mut state = EmoteState::default();
    state.open();
    state.change_emotes();
    let mut listed = Vec::new();
    let pages = state.catalog_page().unwrap().1;
    for _ in 0..pages {
        listed.extend(state.display_slots().into_iter().flatten());
        state.next_catalog_page();
    }
    assert_eq!(listed.len(), clips.len());
    assert!(clips.iter().all(|clip| listed.contains(clip)));
    state.activate_slot(1, 0);
    state.activate_slot(3, 0);
    state.activate_slot(3, 0);
    assert!(state.playback().is_some());
    state.open();
    state.change_emotes();
    state.activate_slot(1, 0);
    assert!(state.is_equipping());
    cosmetics::emotes::activate(&[]);
    state.apply_preferences(None);
    assert!(state.playback().is_none());
    assert!(state.is_choosing());
    assert_eq!(
        state
            .display_slots()
            .into_iter()
            .flatten()
            .collect::<Vec<_>>(),
        Vec::<Item>::new()
    );
}

#[test]
fn fresh_defaults_follow_the_owned_catalog_and_refresh_without_fabricating_emotes() {
    let _catalog = cosmetics::test_support::catalog_lock();
    cosmetics::emotes::activate(&[]);
    let mut state = EmoteState::default();
    assert!(state.slots().iter().all(Option::is_none));
    let free = cosmetics::test_support::playback_emote();
    let paid = cosmetics::test_support::emote("fixture:default_unowned", "Unowned", false);
    cosmetics::emotes::activate(&[paid, free]);
    state.apply_preferences(None);
    assert_eq!(state.slots(), &[Some(free), None, None, None]);
    assert_eq!(EmoteState::default().slots(), state.slots());
    state.open();
    state.activate_slot(0, 0);
    cosmetics::emotes::activate(&[]);
    state.apply_preferences(None);
    assert!(state.slots().iter().all(Option::is_none));
    assert!(state.playback().is_none());
    state.open();
    state.change_emotes();
    assert!(state.display_slots().iter().all(Option::is_none));
}
