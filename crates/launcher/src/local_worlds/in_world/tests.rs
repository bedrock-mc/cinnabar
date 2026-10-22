use super::*;
use crate::local_worlds::options::{OptionChanges, WorldOption as O, WorldOptions};

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
    let effect = change(
        &mut worlds,
        "0123456789abcdef",
        Action::Difficulty(Difficulty::Hard),
    );
    assert_eq!(worlds[0].difficulty, Difficulty::Hard);
    assert_eq!(
        effect,
        Some(Effect::Apply {
            id: "0123456789abcdef".to_owned(),
            update: WorldUpdate {
                difficulty: Some(Difficulty::Hard),
                ..Default::default()
            },
        })
    );
    assert_eq!(
        change(&mut worlds, "missing", Action::Difficulty(Difficulty::Easy)),
        None
    );
    let mut saved = world(Backend::Bds);
    saved.options.apply_changes(&OptionChanges::default());
    saved.difficulty = Difficulty::Peaceful;
    worlds[0].size_bytes = 42;
    applied(&mut worlds, saved);
    assert_eq!(worlds[0].difficulty, Difficulty::Peaceful);
    assert_eq!(worlds[0].size_bytes, 42, "the measured size is kept");
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

/// A change the running server missed stays reported until the world opens again.
#[test]
fn a_missed_live_change_is_reported_until_the_next_open() {
    use crate::local_worlds::model::{Event, Input, WorldsMenu};
    let mut menu = WorldsMenu::default();
    let saved = world(Backend::Bds);
    menu.apply(Event::Listed(vec![saved.clone()]));
    menu.apply(Event::Applied(Applied {
        world: saved.clone(),
        missed: false,
    }));
    assert!(!menu.missed_live(&saved.id));
    menu.apply(Event::Applied(Applied {
        world: saved.clone(),
        missed: true,
    }));
    assert!(menu.missed_live(&saved.id));
    menu.apply(Event::Applied(Applied {
        world: saved.clone(),
        missed: false,
    }));
    assert!(
        menu.missed_live(&saved.id),
        "a later success does not undo the missed change"
    );
    menu.update(Input::Select(0));
    menu.update(Input::Play);
    assert!(!menu.missed_live(&saved.id));
}
