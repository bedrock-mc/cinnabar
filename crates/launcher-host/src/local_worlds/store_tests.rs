use super::*;
use crate::local_worlds::test_support::scratch;

fn store(dir: &tempfile::TempDir) -> Store {
    Store::open(dir.path().join("worlds")).expect("open store")
}

fn new_world(name: &str) -> NewWorld {
    NewWorld {
        name: name.to_owned(),
        ..NewWorld::default()
    }
}

#[test]
fn create_applies_defaults_and_persists() {
    let dir = scratch();
    let store = store(&dir);
    let world = store.create(&new_world("  My World ")).expect("create");
    assert_eq!(world.name, "My World");
    assert_eq!(
        (
            world.game_mode,
            world.generator,
            world.difficulty,
            world.backend
        ),
        (
            GameMode::Survival,
            Generator::Normal,
            Difficulty::Normal,
            Backend::Dragonfly
        )
    );
    assert_eq!(store.get(&world.id).expect("reload"), world);
}

#[test]
fn create_keeps_an_explicit_seed_including_zero() {
    let dir = scratch();
    let store = store(&dir);
    let world = store
        .create(&NewWorld {
            options: Default::default(),
            name: "a".into(),
            seed: Some(0),
            generator: Generator::Flat,
            game_mode: GameMode::Creative,
            difficulty: Difficulty::Hard,
            backend: None,
        })
        .expect("create");
    assert_eq!(world.seed, 0);
    assert_eq!(
        (world.generator, world.game_mode, world.difficulty),
        (Generator::Flat, GameMode::Creative, Difficulty::Hard)
    );
}

#[test]
fn create_rejects_invalid_names() {
    let dir = scratch();
    let store = store(&dir);
    for name in ["", "  ", "bad\0name", "x".repeat(65).as_str()] {
        assert!(
            matches!(store.create(&new_world(name)), Err(WorldError::Invalid(_))),
            "{name:?}"
        );
    }
}

#[test]
fn list_orders_by_last_played_and_skips_junk() {
    let dir = scratch();
    let store = store(&dir);
    store.pin_clock(1000);
    let older = store.create(&new_world("older")).expect("older");
    store.pin_clock(1000 + 3600);
    let newer = store.create(&new_world("newer")).expect("newer");
    fs::create_dir_all(store.root.join("not-an-id")).expect("junk folder");
    let corrupt = store.root.join("0123456789abcdef");
    fs::create_dir_all(&corrupt).expect("corrupt folder");
    fs::write(corrupt.join(META_FILE), b"{").expect("corrupt metadata");
    let ids: Vec<String> = store
        .list()
        .expect("list")
        .into_iter()
        .map(|world| world.id)
        .collect();
    assert_eq!(ids, [newer.id, older.id]);
    store
        .delete("0123456789abcdef")
        .expect("a corrupt world must be deletable");
}

#[test]
fn update_touch_and_delete() {
    let dir = scratch();
    let store = store(&dir);
    let world = store.create(&new_world("a")).expect("create");
    let updated = store
        .update(
            &world.id,
            &WorldUpdate {
                options: None,
                name: Some(" b ".into()),
                game_mode: Some(GameMode::Creative),
                difficulty: Some(Difficulty::Hard),
            },
        )
        .expect("update");
    assert_eq!(
        (updated.name.as_str(), updated.game_mode, updated.difficulty),
        ("b", GameMode::Creative, Difficulty::Hard)
    );
    let empty = WorldUpdate {
        name: Some(String::new()),
        ..WorldUpdate::default()
    };
    assert!(matches!(
        store.update(&world.id, &empty),
        Err(WorldError::Invalid(_))
    ));
    assert_eq!(
        store.get(&world.id).expect("get").name,
        "b",
        "a rejected update changed the world"
    );
    store.pin_clock(5000);
    store.touch(&world.id, &Default::default()).expect("touch");
    assert_eq!(store.get(&world.id).expect("get").last_played_unix, 5000);
    store.delete(&world.id).expect("delete");
    assert!(matches!(store.get(&world.id), Err(WorldError::NotFound)));
    assert!(matches!(store.delete(&world.id), Err(WorldError::NotFound)));
}

#[test]
fn edits_from_play_and_back_round_trip_on_the_next_load() {
    use launcher::local_worlds::{Effect, Event, Input, Screen, WorldsMenu};

    for backend in [Backend::Dragonfly, Backend::Bds] {
        for exit in [Input::PlayFromEdit, Input::Back] {
            let dir = scratch();
            let store = store(&dir);
            let original = store
                .create(&NewWorld {
                    backend: Some(backend),
                    ..new_world("Original")
                })
                .expect("create");
            let mut menu = WorldsMenu::default();
            menu.apply(Event::Listed(vec![original.clone()]));
            menu.update(Input::BeginEdit(0));
            menu.update(Input::SetEditName(" Edited ".to_owned()));
            menu.update(Input::SetGameMode(GameMode::Creative));
            menu.update(Input::SetDifficulty(Difficulty::Hard));

            let effects = menu.update(exit.clone());
            let [Effect::Update { id, update }] = effects.as_slice() else {
                panic!("{exit:?} must save before leaving: {effects:?}");
            };
            assert_eq!(menu.screen(), Screen::Edit);
            let saved = store.update(id, update).expect("save edit");
            let effects = menu.apply(Event::Updated(saved.clone()));
            if exit == Input::PlayFromEdit {
                assert_eq!(effects, vec![Effect::Open(original.id.clone())]);
            } else {
                assert!(effects.is_empty());
                assert_eq!(menu.screen(), Screen::List);
            }

            let reopened = Store::open(store.root.clone()).expect("reopen store");
            let loaded = reopened.get(&original.id).expect("reload edit");
            assert_eq!(loaded, saved);
            assert_eq!(
                (loaded.name.as_str(), loaded.game_mode, loaded.difficulty),
                ("Edited", GameMode::Creative, Difficulty::Hard)
            );
            let mut menu = WorldsMenu::default();
            menu.apply(Event::Listed(reopened.list().expect("reload list")));
            menu.update(Input::BeginEdit(0));
            assert_eq!(
                menu.edit_form(),
                Some(&launcher::local_worlds::form::EditForm::of(&loaded))
            );
        }
    }
}

#[test]
fn malformed_ids_never_reach_the_filesystem() {
    let dir = scratch();
    let store = store(&dir);
    for id in ["", "..", "../x", "ABCDEF0123456789", "0123456789abcde"] {
        assert!(matches!(store.get(id), Err(WorldError::NotFound)), "{id:?}");
        assert!(
            matches!(store.delete(id), Err(WorldError::NotFound)),
            "{id:?}"
        );
    }
}

/// The listing reports each world's size on disk without saving it.
#[test]
fn list_reports_world_size() {
    let dir = scratch();
    let store = store(&dir);
    let world = store.create(&new_world("sized")).expect("create");
    let data = store.dir(&world.id).expect("dir").join(LEVEL_DIR);
    fs::create_dir_all(&data).expect("db folder");
    fs::write(data.join("000001.ldb"), vec![0u8; 4096]).expect("data file");
    let worlds = store.list().expect("list");
    assert!(
        worlds.len() == 1 && worlds[0].size_bytes >= 4096,
        "{worlds:?}"
    );
    store.touch(&world.id, &Default::default()).expect("touch");
    let raw =
        fs::read_to_string(store.dir(&world.id).expect("dir").join(META_FILE)).expect("metadata");
    assert!(!raw.contains("size_bytes"), "{raw}");
}

#[test]
fn backend_and_generator_are_saved_independently() {
    let dir = scratch();
    let store = store(&dir);
    for backend in [Backend::Dragonfly, Backend::Bds] {
        for generator in [Generator::Normal, Generator::Flat] {
            let world = store
                .create(&NewWorld {
                    name: "Independent".into(),
                    backend: Some(backend),
                    generator,
                    ..NewWorld::default()
                })
                .expect("create");
            let saved = store.get(&world.id).expect("get");
            assert_eq!((saved.backend, saved.generator), (backend, generator));
        }
    }
}

/// Worlds saved before backends and generators existed must still list and open.
#[test]
fn legacy_worlds_get_their_backend_defaults() {
    let dir = scratch();
    let store = store(&dir);
    for (backend, generator, expected) in [
        ("", None, (Backend::Dragonfly, Generator::Flat)),
        ("bds", None, (Backend::Bds, Generator::Normal)),
        ("", Some("normal"), (Backend::Dragonfly, Generator::Normal)),
    ] {
        let world = store.create(&new_world("legacy")).expect("create");
        let mut raw = serde_json::json!({"id": world.id, "name": "legacy", "backend": backend});
        if let Some(generator) = generator {
            raw["generator"] = generator.into();
        }
        fs::write(
            store.dir(&world.id).expect("dir").join(META_FILE),
            raw.to_string(),
        )
        .expect("legacy metadata");
        let loaded = store.get(&world.id).expect("legacy world");
        assert_eq!((loaded.backend, loaded.generator), expected, "{raw}");
    }
}

/// `world.json` keeps its existing fields and adds saved option preferences.
#[test]
fn saved_metadata_keeps_its_format() {
    let dir = scratch();
    let store = store(&dir);
    store.pin_clock(42);
    let world = store
        .create(&NewWorld {
            name: "Kept".into(),
            seed: Some(-5),
            backend: Some(Backend::Bds),
            ..NewWorld::default()
        })
        .expect("create");
    let raw =
        fs::read_to_string(store.dir(&world.id).expect("dir").join(META_FILE)).expect("metadata");
    let expected = format!(
        "{{\n  \"id\": \"{}\",\n  \"name\": \"Kept\",\n  \"game_mode\": \"survival\",\n  \"generator\": \"normal\",\n  \"difficulty\": \"normal\",\n  \"backend\": \"bds\",\n  \"seed\": -5,\n  \"options\": {{}},\n  \"pending_options\": {{}},\n  \"created_unix\": 42,\n  \"last_played_unix\": 42\n}}",
        world.id
    );
    assert_eq!(raw, expected);
}

#[test]
fn prefs_persist_and_default_off() {
    let dir = scratch();
    let store = store(&dir);
    assert!(
        !store.prefs().docker_prompt_dismissed,
        "the prompt defaults to shown"
    );
    assert!(
        store
            .update_prefs(true)
            .expect("update")
            .docker_prompt_dismissed
    );
    let reopened = Store::open(store.root.clone()).expect("reopen");
    assert!(
        reopened.prefs().docker_prompt_dismissed,
        "dismissal not saved"
    );
    assert!(
        reopened.list().expect("list").is_empty(),
        "prefs listed as a world"
    );
}

/// A world's captured view is kept beside it and read back whole; worlds that are not saved
/// keep none.
#[test]
fn thumbnails_are_kept_beside_their_world() {
    let dir = scratch();
    let store = store(&dir);
    let world = store.create(&new_world("Pictured")).expect("create");
    assert_eq!(store.thumbnail(&world.id), None);
    store
        .save_thumbnail(&world.id, b"first view")
        .expect("save a thumbnail");
    store
        .save_thumbnail(&world.id, b"later view")
        .expect("replace it");
    assert_eq!(
        store.thumbnail(&world.id).as_deref(),
        Some(&b"later view"[..])
    );
    assert!(matches!(
        store.save_thumbnail("0123456789abcdef", b"view"),
        Err(WorldError::NotFound)
    ));
}

/// Opening the store, which happens on a frame, leaves import folders a killed client left
/// behind to the worker; reclaiming them removes those with an unlocked or missing lock file but
/// never a live import's folder and lock.
#[test]
fn abandoned_import_folders_are_reclaimed_off_the_open() {
    let dir = scratch();
    let root = dir.path().join("worlds");
    let store = store(&dir);
    let live = store.staging().expect("live import");
    for (name, with_lock_file) in [("unlocked", true), ("missing", false)] {
        let folder = root.join(format!("{STAGING_PREFIX}{name}"));
        fs::create_dir_all(folder.join("db")).expect("partial world");
        if with_lock_file {
            fs::write(
                root.join(format!("{STAGING_PREFIX}{name}{STAGING_LOCK_SUFFIX}")),
                b"",
            )
            .expect("lock file");
        }
    }
    let listing = || {
        let mut left: Vec<_> = fs::read_dir(&root)
            .expect("worlds")
            .map(|entry| entry.expect("entry").path())
            .collect();
        left.sort();
        left
    };
    let before = listing();
    let reopened = Store::open(root.clone()).expect("reopen");
    assert_eq!(
        listing(),
        before,
        "opening must not walk or delete import folders"
    );
    reopened.reclaim_abandoned_imports();
    let mut expected = vec![live.path().to_path_buf(), live.lock_path.clone()];
    expected.sort();
    assert_eq!(listing(), expected);
}

#[test]
fn world_options_persist_every_value_through_create_and_edit() {
    use launcher::local_worlds::options::{Control, WorldOption as O};
    let dir = scratch();
    let store = store(&dir);
    for &id in O::ALL {
        let definition = id.definition();
        let value = match definition.control {
            Control::Toggle => 1 - definition.default,
            Control::Choice(choices) => {
                choices
                    .iter()
                    .find(|choice| choice.value != definition.default)
                    .unwrap()
                    .value
            }
            Control::Number { min, max, .. } => {
                if min == definition.default {
                    max
                } else {
                    min
                }
            }
        };
        let mut new = new_world("Options");
        new.options.set(id, value);
        let world = store.create(&new).unwrap();
        let reopened = store.get(&world.id).unwrap();
        assert_eq!(reopened.options, new.options, "{id:?}");
        let edit = launcher::local_worlds::form::EditForm::of(&reopened);
        assert_eq!(edit.options.get(id), value, "{id:?}");
        let mut edited = edit.options;
        edited.set(id, definition.default);
        store
            .update(
                &world.id,
                &WorldUpdate {
                    options: Some(edited.changes_from(&reopened.options)),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(store.get(&world.id).unwrap().options, edited);
    }
}

#[test]
fn world_options_reopen_server_saved_rules_and_keep_pending_menu_edits() {
    use launcher::local_worlds::options::WorldOption as O;
    let dir = scratch();
    let store = store(&dir);
    let mut new = new_world("Rules");
    new.backend = Some(Backend::Bds);
    new.options.set(O::KeepInventory, 1);
    let original = store.create(&new).unwrap();
    let level = store.dir(&original.id).unwrap().join(LEVEL_DIR);
    fs::create_dir_all(&level).unwrap();
    crate::local_worlds::test_support::write_created_level_dat(&level);
    super::super::level_dat::configure(&original, &level, true).unwrap();
    store.touch(&original.id, &Default::default()).unwrap();
    let mut played = original.clone();
    played.options.set(O::RandomTickSpeed, 42);
    super::super::level_dat::configure(&played, &level, false).unwrap();
    let reopened = store.get(&original.id).unwrap();
    assert_eq!(reopened.options.get(O::RandomTickSpeed), 42);
    let baseline = reopened.options.clone();
    let mut edited = reopened.options;
    edited.set(O::RandomTickSpeed, 7);
    store
        .update(
            &original.id,
            &WorldUpdate {
                options: Some(edited.changes_from(&baseline)),
                ..Default::default()
            },
        )
        .unwrap();
    store
        .update(
            &original.id,
            &WorldUpdate {
                name: Some("Renamed".into()),
                ..Default::default()
            },
        )
        .unwrap();
    let (pending, applied, _) = store.snapshot(&original.id).unwrap();
    assert_eq!(pending.options.get(O::RandomTickSpeed), 7);
    super::super::level_dat::configure(&pending, &level, false).unwrap();
    store.touch(&original.id, &applied).unwrap();
    assert_eq!(
        store
            .get(&original.id)
            .unwrap()
            .options
            .get(O::RandomTickSpeed),
        7
    );
}

#[test]
fn world_options_inactive_preferences_survive_configuration_and_reload() {
    use launcher::local_worlds::options::WorldOption as O;
    let dir = scratch();
    let store = store(&dir);
    let mut new = new_world("Preferences");
    new.options.set(O::SleepPercentage, 35);
    new.options.set(O::SleepSkipNight, 0);
    new.options.set(O::Multiplayer, 0);
    let world = store.create(&new).unwrap();
    let level = store.dir(&world.id).unwrap().join(LEVEL_DIR);
    fs::create_dir_all(&level).unwrap();
    crate::local_worlds::test_support::write_created_level_dat(&level);
    super::super::level_dat::configure(&world, &level, true).unwrap();
    store.touch(&world.id, &Default::default()).unwrap();
    let loaded = store.get(&world.id).unwrap();
    assert_eq!(loaded.options.get(O::SleepPercentage), 35);
    assert!(!loaded.options.enabled(O::SleepSkipNight));
    assert!(!loaded.options.enabled(O::Multiplayer));
    let mut server = loaded;
    server.options.set(O::Multiplayer, 1);
    server.options.set(O::FireSpreads, 0);
    super::super::level_dat::configure(&server, &level, false).unwrap();
    let loaded = store.get(&world.id).unwrap();
    assert!(!loaded.options.enabled(O::Multiplayer));
    assert!(!loaded.options.enabled(O::FireSpreads));
}

#[test]
fn world_options_menu_deltas_preserve_rules_changed_after_the_cached_list() {
    use launcher::local_worlds::{Effect, Event, Input, WorldsMenu, options::WorldOption as O};
    let dir = scratch();
    let store = store(&dir);
    let mut new = new_world("Current rules");
    new.backend = Some(Backend::Bds);
    let original = store.create(&new).unwrap();
    let level = store.dir(&original.id).unwrap().join(LEVEL_DIR);
    fs::create_dir_all(&level).unwrap();
    crate::local_worlds::test_support::write_created_level_dat(&level);
    super::super::level_dat::configure(&original, &level, true).unwrap();
    store.touch(&original.id, &Default::default()).unwrap();
    let mut menu = WorldsMenu::default();
    menu.apply(Event::Listed(vec![original.clone()]));
    let mut played = original.clone();
    played.options.set(O::RandomTickSpeed, 42);
    super::super::level_dat::configure(&played, &level, false).unwrap();
    menu.update(Input::BeginEdit(0));
    menu.update(Input::SetOption(O::ShowCoordinates, 1));
    let effects = menu.update(Input::SubmitEdit);
    let Effect::Update { id, update } = &effects[0] else {
        panic!("settings update");
    };
    assert_eq!(
        update.options.as_ref().unwrap().get(O::RandomTickSpeed),
        None
    );
    let saved = store.update(id, update).unwrap();
    assert_eq!(saved.options.get(O::RandomTickSpeed), 42);
    assert!(saved.options.enabled(O::ShowCoordinates));
}

/// A world whose level data exceeds the read bound reports its options unavailable and
/// keeps that file byte-identical through opening and configuration.
#[test]
fn oversized_level_data_marks_options_unavailable_and_stays_untouched() {
    use launcher::local_worlds::options::WorldOption as O;
    let dir = scratch();
    let store = store(&dir);
    let mut new = new_world("Oversized");
    new.options.set(O::ShowCoordinates, 1);
    let world = store.create(&new).unwrap();
    let level = store.dir(&world.id).unwrap().join(LEVEL_DIR);
    fs::create_dir_all(&level).unwrap();
    crate::local_worlds::test_support::write_created_level_dat(&level);
    super::super::level_dat::configure(&world, &level, true).unwrap();
    assert!(!store.get(&world.id).unwrap().options_unavailable);
    let path = level.join("level.dat");
    let oversized = crate::local_worlds::test_support::padded_level_dat(
        &fs::read(&path).unwrap(),
        super::super::level_dat::MAX_LEVEL_DAT_BYTES as usize,
    );
    fs::write(&path, &oversized).unwrap();
    let opened = store.get(&world.id).unwrap();
    assert!(opened.options_unavailable);
    assert!(opened.options.enabled(O::ShowCoordinates));
    super::super::level_dat::configure(&opened, &level, false).unwrap();
    assert!(fs::read(&path).unwrap() == oversized);
}

/// An edit saved while a world starts outlives that start's touch and applies on the next read.
#[test]
fn world_options_edits_saved_during_a_start_stay_pending() {
    use launcher::local_worlds::options::WorldOption as O;
    let dir = scratch();
    let store = store(&dir);
    let mut new = new_world("Racing");
    new.backend = Some(Backend::Bds);
    let original = store.create(&new).unwrap();
    let level = store.dir(&original.id).unwrap().join(LEVEL_DIR);
    fs::create_dir_all(&level).unwrap();
    crate::local_worlds::test_support::write_created_level_dat(&level);
    super::super::level_dat::configure(&original, &level, true).unwrap();
    let edit = |value: i32| {
        let baseline = store.get(&original.id).unwrap().options;
        let mut edited = baseline.clone();
        edited.set(O::RandomTickSpeed, value);
        store
            .update(
                &original.id,
                &WorldUpdate {
                    options: Some(edited.changes_from(&baseline)),
                    ..Default::default()
                },
            )
            .unwrap();
    };
    edit(7);
    let (starting, applied, _) = store.snapshot(&original.id).unwrap();
    super::super::level_dat::configure(&starting, &level, false).unwrap();
    edit(9);
    store.touch(&original.id, &applied).unwrap();
    assert_eq!(
        store
            .get(&original.id)
            .unwrap()
            .options
            .get(O::RandomTickSpeed),
        9
    );
}

/// A first start that left BDS's level data but never applied the options still applies the
/// creation-only settings and requested rules on the retry.
#[test]
fn world_options_survive_a_first_start_that_failed_after_bds_created_the_world() {
    use launcher::local_worlds::options::WorldOption as O;
    let dir = scratch();
    let store = store(&dir);
    let mut new = new_world("Retried");
    new.backend = Some(Backend::Bds);
    for id in [O::StartingMap, O::BonusChest, O::KeepInventory] {
        new.options.set(id, 1);
    }
    let created = store.create(&new).unwrap();
    let level = store.dir(&created.id).unwrap().join(LEVEL_DIR);
    fs::create_dir_all(&level).unwrap();
    crate::local_worlds::test_support::write_created_level_dat(&level);
    let (world, _, unapplied) = store.snapshot(&created.id).unwrap();
    assert!(unapplied);
    assert!(world.options.enabled(O::KeepInventory));
    super::super::level_dat::configure(&world, &level, unapplied).unwrap();
    let written = super::super::level_dat::try_read_options(&level.join("level.dat")).unwrap();
    for id in [O::StartingMap, O::BonusChest, O::KeepInventory] {
        assert!(written.enabled(id), "{id:?}");
    }
    store.touch(&created.id, &Default::default()).unwrap();
    assert!(!store.snapshot(&created.id).unwrap().2);
}
