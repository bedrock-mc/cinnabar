use super::*;
use crate::local_worlds::model::Effect;
use crate::local_worlds::options::{OptionChanges, WorldOption as O, WorldOptions};

/// A saved world with default settings for edit reconciliation.
fn world(backend: Backend) -> World {
    World {
        id: "0123456789abcdef".to_owned(),
        name: "Home".to_owned(),
        game_mode: GameMode::Survival,
        generator: Default::default(),
        difficulty: Difficulty::Normal,
        backend,
        seed: 1,
        options: WorldOptions::default(),
        options_unavailable: false,
        created_unix: 0,
        last_played_unix: 0,
        size_bytes: 0,
    }
}

/// Creation-only options cannot change in a running world; rules and cheats apply at once.
#[test]
fn timing_follows_where_the_server_reads_each_option() {
    for &id in O::ALL {
        let definition = id.definition();
        let expected = if definition.creation_only || id == O::SimulationDistance {
            Timing::Locked
        } else if id == O::ActivateCheats
            || matches!(definition.storage, Storage::Rule(_) | Storage::Combined)
        {
            Timing::Live
        } else {
            Timing::NextStart
        };
        assert_eq!(timing(id), expected, "{id:?}");
    }
    assert_eq!(timing(O::BonusChest), Timing::Locked);
    assert_eq!(timing(O::KeepInventory), Timing::Live);
    assert_eq!(timing(O::PlayerPermissions), Timing::NextStart);
}

#[test]
fn unchanged_and_locked_settings_make_no_update() {
    let bds = world(Backend::Bds);
    assert_eq!(update(&bds, Action::Difficulty(Difficulty::Normal)), None);
    assert_eq!(update(&bds, Action::GameMode(GameMode::Survival)), None);
    assert_eq!(update(&bds, Action::Option(O::BonusChest, 1)), None);
    assert_eq!(update(&bds, Action::Option(O::SimulationDistance, 8)), None);
    assert_eq!(update(&bds, Action::Option(O::RespawnRadius, -1)), None);
    assert_eq!(update(&bds, Action::OptionField(O::RespawnRadius)), None);
    assert_eq!(
        update(&world(Backend::Dragonfly), Action::Option(O::Pvp, 0)),
        None,
        "worlds without options change none"
    );
}

/// The change shows at once and becomes one save-and-apply effect for that world.
#[test]
fn a_change_shows_before_it_is_saved() {
    let mut worlds = vec![world(Backend::Bds)];
    let mut edits = Edits::default();
    let effect = edits.change(
        &mut worlds,
        "0123456789abcdef",
        Action::Difficulty(Difficulty::Hard),
    );
    assert_eq!(worlds[0].difficulty, Difficulty::Hard);
    assert_eq!(
        effect,
        Some(Effect::Apply {
            id: "0123456789abcdef".to_owned(),
            revision: 1,
            update: WorldUpdate {
                difficulty: Some(Difficulty::Hard),
                ..Default::default()
            },
        })
    );
    assert_eq!(
        edits.change(&mut worlds, "missing", Action::Difficulty(Difficulty::Easy)),
        None
    );
    let mut saved = world(Backend::Bds);
    saved.options.apply_changes(&OptionChanges::default());
    saved.difficulty = Difficulty::Peaceful;
    worlds[0].size_bytes = 42;
    edits.finish(&mut worlds, &saved.id, 1, Ok(&saved));
    assert_eq!(worlds[0].difficulty, Difficulty::Peaceful);
    assert_eq!(worlds[0].size_bytes, 42, "the measured size is kept");
}

/// Every successful save refreshes server values, including options the menu did not edit.
#[test]
fn successful_saves_refresh_authoritative_world_options() {
    use crate::local_worlds::model::{Event, Input, WorldsMenu};
    for previously_edited in [false, true] {
        let mut menu = WorldsMenu::default();
        let mut saved = world(Backend::Bds);
        let id = saved.id.clone();
        menu.apply(Event::Listed(vec![saved.clone()]));
        if previously_edited {
            let effect = menu
                .update(Input::InWorld(id.clone(), Action::Option(O::Pvp, 0)))
                .pop()
                .unwrap();
            menu.apply(saved_reply(effect, &mut saved));
        }
        let server_value = i32::from(previously_edited);
        saved.options.set(O::Pvp, server_value);
        let effect = menu
            .update(Input::InWorld(
                id.clone(),
                Action::Difficulty(Difficulty::Hard),
            ))
            .pop()
            .unwrap();
        menu.apply(saved_reply(effect, &mut saved));
        assert_eq!(
            menu.in_world_view(&id).unwrap().world.options,
            saved.options
        );
        assert_eq!(
            menu.update(Input::InWorld(id, Action::Option(O::Pvp, 1 - server_value)))
                .len(),
            1,
            "the next toggle saves a different value"
        );
    }
}

/// A pending edit overlays the latest server snapshot and rolls back to it if saving fails.
#[test]
fn newer_pending_edits_roll_back_to_authoritative_values() {
    use crate::local_worlds::model::{Event, Input, WorldsMenu};
    let mut menu = WorldsMenu::default();
    let mut saved = world(Backend::Bds);
    let id = saved.id.clone();
    menu.apply(Event::Listed(vec![saved.clone()]));
    let difficulty = menu
        .update(Input::InWorld(
            id.clone(),
            Action::Difficulty(Difficulty::Hard),
        ))
        .pop()
        .unwrap();
    let radius = menu
        .update(Input::InWorld(
            id.clone(),
            Action::Option(O::RespawnRadius, 24),
        ))
        .pop()
        .unwrap();
    saved.options.set(O::RespawnRadius, 30);
    menu.apply(saved_reply(difficulty, &mut saved));
    assert_eq!(menu.worlds()[0].options.get(O::RespawnRadius), 24);
    menu.apply(failed_reply(radius, "radius save failed"));
    assert_eq!(menu.worlds()[0].options.get(O::RespawnRadius), 30);
}

/// Refreshing other settings does not hide a failure that arrives after the snapshot.
#[test]
fn authoritative_refresh_preserves_delayed_unrelated_failures() {
    use crate::local_worlds::model::{Event, Input, WorldsMenu};
    let mut menu = WorldsMenu::default();
    let mut saved = world(Backend::Bds);
    let id = saved.id.clone();
    menu.apply(Event::Listed(vec![saved.clone()]));
    let radius = menu
        .update(Input::InWorld(
            id.clone(),
            Action::Option(O::RespawnRadius, 24),
        ))
        .pop()
        .unwrap();
    let difficulty = menu
        .update(Input::InWorld(
            id.clone(),
            Action::Difficulty(Difficulty::Hard),
        ))
        .pop()
        .unwrap();
    menu.apply(saved_reply(difficulty, &mut saved));
    assert_eq!(menu.worlds()[0].options, saved.options);
    menu.apply(failed_reply(radius, "radius save failed"));
    assert_eq!(menu.worlds()[0].options, saved.options);
    assert_eq!(
        menu.in_world_view(&id).unwrap().error.as_deref(),
        Some("radius save failed")
    );
}

/// Delayed replies cannot replace a newer snapshot, even for options first edited afterward.
#[test]
fn older_saves_do_not_replace_a_newer_authoritative_world() {
    use crate::local_worlds::model::{Event, Input, WorldsMenu};
    let mut menu = WorldsMenu::default();
    let mut saved = world(Backend::Bds);
    let id = saved.id.clone();
    menu.apply(Event::Listed(vec![saved.clone()]));
    let mut replies = Vec::new();
    for difficulty in [Difficulty::Hard, Difficulty::Easy] {
        let effect = menu
            .update(Input::InWorld(id.clone(), Action::Difficulty(difficulty)))
            .pop()
            .unwrap();
        saved
            .options
            .set(O::Pvp, i32::from(difficulty == Difficulty::Hard));
        replies.push(saved_reply(effect, &mut saved));
    }
    menu.apply(replies.pop().unwrap());
    let radius = menu
        .update(Input::InWorld(id, Action::Option(O::RespawnRadius, 24)))
        .pop()
        .unwrap();
    menu.apply(replies.pop().unwrap());
    assert_eq!(menu.worlds()[0].difficulty, Difficulty::Easy);
    assert_eq!(menu.worlds()[0].options.get(O::Pvp), 0);
    assert_eq!(menu.worlds()[0].options.get(O::RespawnRadius), 24);
    menu.apply(failed_reply(radius, "radius save failed"));
    assert_eq!(menu.worlds()[0].options, saved.options);
}

/// Every catalog option is placed once, so a new option cannot silently miss the section.
#[test]
fn the_game_section_places_every_option_once() {
    for &id in O::ALL {
        let shown = GAME_SECTION
            .iter()
            .filter(|row| matches!(row, Row::Option(option, ..) if *option == id))
            .count();
        let outside = OUTSIDE_GAME_SECTION.contains(&id);
        assert_eq!(
            (shown, outside),
            if outside { (0, true) } else { (1, false) },
            "{id:?}"
        );
    }
}

/// Without cheats the cheat rules wait for them; turning cheats off keeps the rules and drops an
/// operator default to member.
#[test]
fn cheat_rules_follow_the_running_world_switch() {
    use crate::local_worlds::options::{MEMBER, OPERATOR};
    let mut world = world(Backend::Bds);
    assert_eq!(update(&world, Action::Option(O::KeepInventory, 1)), None);
    let on = update(&world, Action::Option(O::ActivateCheats, 1)).expect("cheats on");
    apply(&mut world, &on);
    let keep = update(&world, Action::Option(O::KeepInventory, 1)).expect("a rule");
    apply(&mut world, &keep);
    world.options.set(O::PlayerPermissions, OPERATOR);
    let off = update(&world, Action::Option(O::ActivateCheats, 0)).expect("cheats off");
    apply(&mut world, &off);
    assert!(
        world.options.enabled(O::KeepInventory),
        "rules keep their values"
    );
    assert_eq!(world.options.get(O::PlayerPermissions), MEMBER);
}

/// A creative default game mode turns cheats on with it.
#[test]
fn a_creative_default_activates_cheats() {
    let world = world(Backend::Bds);
    let update = update(&world, Action::GameMode(GameMode::Creative)).expect("an update");
    assert_eq!(update.game_mode, Some(GameMode::Creative));
    assert_eq!(
        update
            .options
            .and_then(|changes| changes.get(O::ActivateCheats)),
        Some(1)
    );
}

/// An older acknowledgement must not hide the latest choice or suppress a later retry.
#[test]
fn hard_easy_hard_keeps_the_last_selection() {
    use crate::local_worlds::model::{Event, Input, WorldsMenu};
    let mut menu = WorldsMenu::default();
    let mut saved = world(Backend::Bds);
    let id = saved.id.clone();
    menu.apply(Event::Listed(vec![saved.clone()]));
    let hard = menu.update(Input::InWorld(
        id.clone(),
        Action::Difficulty(Difficulty::Hard),
    ));
    let easy = menu.update(Input::InWorld(
        id.clone(),
        Action::Difficulty(Difficulty::Easy),
    ));
    menu.apply(saved_reply(hard.into_iter().next().unwrap(), &mut saved));
    assert_eq!(menu.worlds()[0].difficulty, Difficulty::Easy);
    let hard = menu.update(Input::InWorld(id, Action::Difficulty(Difficulty::Hard)));
    assert_eq!(hard.len(), 1, "the last Hard selection must be saved");
    menu.apply(saved_reply(easy.into_iter().next().unwrap(), &mut saved));
    assert_eq!(menu.worlds()[0].difficulty, Difficulty::Hard);
    menu.apply(saved_reply(hard.into_iter().next().unwrap(), &mut saved));
    assert_eq!(saved.difficulty, Difficulty::Hard);
    assert_eq!(menu.worlds()[0].difficulty, Difficulty::Hard);
}

/// Saves a queued update and returns the worker's successful reply.
fn saved_reply(effect: Effect, saved: &mut World) -> crate::local_worlds::model::Event {
    let Effect::Apply {
        revision, update, ..
    } = effect
    else {
        panic!("a save-and-apply effect")
    };
    apply(saved, &update);
    crate::local_worlds::model::Event::Applied {
        revision,
        applied: Applied {
            world: saved.clone(),
            missed: false,
        },
    }
}

/// Returns the worker's failed reply for a queued save.
fn failed_reply(effect: Effect, error: &str) -> crate::local_worlds::model::Event {
    let Effect::Apply { id, revision, .. } = effect else {
        panic!("a save-and-apply effect")
    };
    crate::local_worlds::model::Event::ApplyFailed {
        id,
        revision,
        error: error.into(),
    }
}

/// A newer successful value clears an older failure, regardless of reply order or setting type.
#[test]
fn successful_edits_clear_superseded_failures() {
    use crate::local_worlds::model::{Event, Input, WorldsMenu};
    for (first, second) in [
        (
            Action::Option(O::RespawnRadius, 2),
            Action::Option(O::RespawnRadius, 24),
        ),
        (
            Action::Difficulty(Difficulty::Hard),
            Action::Difficulty(Difficulty::Easy),
        ),
        (
            Action::GameMode(GameMode::Creative),
            Action::GameMode(GameMode::Survival),
        ),
    ] {
        for failure_first in [true, false] {
            let mut menu = WorldsMenu::default();
            let mut saved = world(Backend::Bds);
            // Keep the game-mode edits independent of implicit cheat changes.
            saved.options.restore_cheats(true);
            let id = saved.id.clone();
            menu.apply(Event::Listed(vec![saved.clone()]));
            let first = menu
                .update(Input::InWorld(id.clone(), first))
                .pop()
                .unwrap();
            let second_save = menu
                .update(Input::InWorld(id.clone(), second))
                .pop()
                .unwrap();
            let failure = failed_reply(first, "Couldn't save the change");
            let success = saved_reply(second_save, &mut saved);
            if failure_first {
                menu.apply(failure);
                assert!(menu.in_world_view(&id).unwrap().error.is_some());
                menu.apply(success);
            } else {
                menu.apply(success);
                menu.apply(failure);
            }
            let view = menu.in_world_view(&id).unwrap();
            assert_eq!(view.world, saved);
            assert_eq!(view.error, None);
            assert!(menu.update(Input::InWorld(id, second)).is_empty());
        }
    }
}

/// Recovering one failed setting keeps the other setting's failure visible until it saves too.
#[test]
fn successful_edits_preserve_unrelated_failures() {
    use crate::local_worlds::model::{Event, Input, WorldsMenu};
    let mut menu = WorldsMenu::default();
    let mut saved = world(Backend::Bds);
    let id = saved.id.clone();
    menu.apply(Event::Listed(vec![saved.clone()]));
    let radius = Action::Option(O::RespawnRadius, 24);
    let difficulty = Action::Difficulty(Difficulty::Hard);
    let first = menu
        .update(Input::InWorld(id.clone(), radius))
        .pop()
        .unwrap();
    let second = menu
        .update(Input::InWorld(id.clone(), difficulty))
        .pop()
        .unwrap();
    menu.apply(failed_reply(first, "radius save failed"));
    menu.apply(failed_reply(second, "difficulty save failed"));
    assert_eq!(
        menu.in_world_view(&id).unwrap().error.as_deref(),
        Some("difficulty save failed")
    );
    for (action, remaining) in [(difficulty, Some("radius save failed")), (radius, None)] {
        let retry = menu
            .update(Input::InWorld(id.clone(), action))
            .pop()
            .unwrap();
        assert!(
            menu.in_world_view(&id).unwrap().error.is_some(),
            "a retry has not saved yet"
        );
        menu.apply(saved_reply(retry, &mut saved));
        assert_eq!(menu.in_world_view(&id).unwrap().error.as_deref(), remaining);
    }
}

/// An already acknowledged or previous-visit reply cannot introduce a new save error.
#[test]
fn untracked_failures_do_not_show_errors() {
    let mut worlds = vec![world(Backend::Bds)];
    let id = worlds[0].id.clone();
    let mut edits = Edits::default();
    let action = Action::Option(O::RespawnRadius, 24);
    edits.change(&mut worlds, &id, action).unwrap();
    let saved = worlds[0].clone();
    edits.finish(&mut worlds, &id, 1, Ok(&saved));
    edits.finish(&mut worlds, &id, 1, Err("duplicate failure".into()));
    assert_eq!(edits.view(&worlds[0]).unwrap().error, None);
    edits.reset();
    edits
        .change(&mut worlds, &id, Action::Option(O::RespawnRadius, 25))
        .unwrap();
    edits.finish(&mut worlds, &id, 1, Err("previous visit's failure".into()));
    assert_eq!(edits.view(&worlds[0]).unwrap().error, None);
}

/// A failed save restores its confirmed fields while preserving unrelated pending edits.
#[test]
fn failed_edits_roll_back_to_the_last_successful_save() {
    use crate::local_worlds::model::{Event, Input, Screen, WorldsMenu};
    let mut menu = WorldsMenu::default();
    let mut saved = world(Backend::Bds);
    let id = saved.id.clone();
    menu.apply(Event::Listed(vec![saved.clone()]));
    let hard = menu
        .update(Input::InWorld(
            id.clone(),
            Action::Difficulty(Difficulty::Hard),
        ))
        .pop()
        .unwrap();
    menu.apply(saved_reply(hard, &mut saved));
    let easy = menu
        .update(Input::InWorld(
            id.clone(),
            Action::Difficulty(Difficulty::Easy),
        ))
        .pop()
        .unwrap();
    let pvp = menu
        .update(Input::InWorld(id.clone(), Action::Option(O::Pvp, 0)))
        .pop()
        .unwrap();
    let Effect::Apply { revision, .. } = easy else {
        panic!("save")
    };
    menu.apply(Event::ApplyFailed {
        id: id.clone(),
        revision,
        error: "Couldn't save the world".into(),
    });
    let view = menu.in_world_view(&id).unwrap();
    assert_eq!(view.world.difficulty, Difficulty::Hard);
    assert!(!view.world.options.enabled(O::Pvp));
    assert!(view.error.is_some());
    assert!(
        !view.missed,
        "an unsaved edit is not saved for the next start"
    );
    assert_eq!(menu.screen(), Screen::List);
    menu.apply(saved_reply(pvp, &mut saved));
    assert_eq!(menu.worlds()[0].difficulty, Difficulty::Hard);
    assert!(menu.in_world_view(&id).unwrap().error.is_some());
    let retry = menu
        .update(Input::InWorld(
            id.clone(),
            Action::Difficulty(Difficulty::Easy),
        ))
        .pop()
        .unwrap();
    menu.apply(saved_reply(retry, &mut saved));
    assert_eq!(menu.worlds()[0].difficulty, Difficulty::Easy);
    assert!(menu.in_world_view(&id).unwrap().error.is_none());
}

/// Each option keeps its own revisions, including implicit edits made by dependencies.
#[test]
fn older_option_replies_keep_pending_and_confirmed_values() {
    use crate::local_worlds::model::{Event, Input, WorldsMenu};
    let mut menu = WorldsMenu::default();
    let mut saved = world(Backend::Bds);
    let id = saved.id.clone();
    menu.apply(Event::Listed(vec![saved.clone()]));
    let creative = menu
        .update(Input::InWorld(
            id.clone(),
            Action::GameMode(GameMode::Creative),
        ))
        .pop()
        .unwrap();
    let off = menu
        .update(Input::InWorld(
            id.clone(),
            Action::Option(O::ActivateCheats, 0),
        ))
        .pop()
        .unwrap();
    let radius = menu
        .update(Input::InWorld(
            id.clone(),
            Action::Option(O::RespawnRadius, 24),
        ))
        .pop()
        .unwrap();
    let creative = saved_reply(creative, &mut saved);
    let off = saved_reply(off, &mut saved);
    let radius = saved_reply(radius, &mut saved);
    menu.apply(off);
    menu.apply(creative);
    assert_eq!(menu.worlds()[0].game_mode, GameMode::Creative);
    assert!(!menu.worlds()[0].options.enabled(O::ActivateCheats));
    assert_eq!(menu.worlds()[0].options.get(O::RespawnRadius), 24);
    menu.apply(radius);
    assert_eq!(menu.worlds()[0], saved);
}

/// A change the running server missed stays reported until the world opens again.
#[test]
fn a_missed_live_change_is_reported_until_the_next_open() {
    use crate::local_worlds::model::{Event, Input, WorldsMenu};
    let mut menu = WorldsMenu::default();
    let saved = world(Backend::Bds);
    menu.apply(Event::Listed(vec![saved.clone()]));
    menu.apply(Event::Applied {
        revision: 0,
        applied: Applied {
            world: saved.clone(),
            missed: false,
        },
    });
    assert!(!menu.missed_live(&saved.id));
    menu.apply(Event::Applied {
        revision: 0,
        applied: Applied {
            world: saved.clone(),
            missed: true,
        },
    });
    assert!(menu.missed_live(&saved.id));
    menu.apply(Event::Applied {
        revision: 0,
        applied: Applied {
            world: saved.clone(),
            missed: false,
        },
    });
    assert!(
        menu.missed_live(&saved.id),
        "a later success does not undo the missed change"
    );
    menu.update(Input::Select(0));
    menu.update(Input::Play);
    assert!(!menu.missed_live(&saved.id));
}
