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
    store.touch(&world.id).expect("touch");
    assert_eq!(store.get(&world.id).expect("get").last_played_unix, 5000);
    store.delete(&world.id).expect("delete");
    assert!(matches!(store.get(&world.id), Err(WorldError::NotFound)));
    assert!(matches!(store.delete(&world.id), Err(WorldError::NotFound)));
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
    let data = store.dir(&world.id).expect("dir").join("db");
    fs::create_dir_all(&data).expect("db folder");
    fs::write(data.join("000001.ldb"), vec![0u8; 4096]).expect("data file");
    let worlds = store.list().expect("list");
    assert!(
        worlds.len() == 1 && worlds[0].size_bytes >= 4096,
        "{worlds:?}"
    );
    store.touch(&world.id).expect("touch");
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

/// `world.json` keeps the field names and order earlier releases wrote.
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
        "{{\n  \"id\": \"{}\",\n  \"name\": \"Kept\",\n  \"game_mode\": \"survival\",\n  \"generator\": \"normal\",\n  \"difficulty\": \"normal\",\n  \"backend\": \"bds\",\n  \"seed\": -5,\n  \"created_unix\": 42,\n  \"last_played_unix\": 42\n}}",
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
