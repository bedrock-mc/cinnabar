use super::*;
use crate::local_worlds::test_support;

/// Returns the payload of a named root tag in generated level data.
fn payload<'a>(fields: &'a Fields, name: &str) -> (u8, &'a [u8]) {
    let (_, bytes) = fields
        .iter()
        .find(|(key, _)| key == name.as_bytes())
        .unwrap();
    (bytes[0], &bytes[3 + name.len()..])
}

#[test]
fn world_options_generate_every_level_setting_and_rule() {
    for &id in O::ALL {
        let mut world = test_support::world();
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
                if definition.default == min {
                    max
                } else {
                    min
                }
            }
        };
        world.options.set(id, value);
        let bytes = configured(&world, &[]).unwrap();
        let fields = read_fields(&bytes[8..]).unwrap();
        if let Storage::Rule(key) | Storage::Level(key) = definition.storage {
            let (kind, got) = payload(&fields, key);
            match definition.control {
                Control::Toggle => assert_eq!((kind, got), (1, &[value as u8][..]), "{id:?}"),
                Control::Number { .. } | Control::Choice(_) => {
                    assert_eq!((kind, got), (3, &value.to_le_bytes()[..]), "{id:?}")
                }
            }
        }
    }
}

#[test]
fn world_options_compose_sleep_cheats_and_always_day() {
    let mut world = test_support::world();
    world.options.set(O::SleepSkipNight, 0);
    world.options.set(O::AlwaysDay, 1);
    let bytes = configured(&world, &[]).unwrap();
    let fields = read_fields(&bytes[8..]).unwrap();
    assert_eq!(
        payload(&fields, "playerssleepingpercentage"),
        (3, &101i32.to_le_bytes()[..])
    );
    assert_eq!(payload(&fields, "dodaylightcycle"), (1, &[0][..]));
    assert_eq!(payload(&fields, "Time"), (4, &6000i64.to_le_bytes()[..]));
    assert_eq!(payload(&fields, "cheatsEnabled"), (1, &[1][..]));
    world.options.set(O::ActivateCheats, 0);
    let saved = configured(&world, &bytes).unwrap();
    let fields = read_fields(&saved[8..]).unwrap();
    assert_eq!(payload(&fields, "cheatsEnabled"), (1, &[0][..]));
    assert_eq!(payload(&fields, "hasBeenLoadedInCreative"), (1, &[1][..]));
}

#[test]
fn world_options_preserve_unknown_tags_and_creation_only_settings_on_reload() {
    let mut world = test_support::world();
    world.options.set(O::BonusChest, 1);
    let bytes = configured(&world, &[]).unwrap();
    let mut fields = read_fields(&bytes[8..]).unwrap();
    let opaque = [10, 1, 0, b'x', 1, 2, 0, b'a', b'b', 7, 0];
    fields.push((b"x".to_vec(), opaque.to_vec()));
    let mut body = vec![10, 0, 0];
    for (_, field) in fields {
        body.extend(field);
    }
    body.push(0);
    let mut old = 9u32.to_le_bytes().to_vec();
    old.extend((body.len() as u32).to_le_bytes());
    old.extend(body);
    world.options.set(O::BonusChest, 0);
    world.options.set(O::ShowCoordinates, 1);
    let new = configured(&world, &old).unwrap();
    assert_eq!(&new[..4], &old[..4]);
    let fields = read_fields(&new[8..]).unwrap();
    assert_eq!(
        fields.iter().find(|(key, _)| key == b"x").unwrap().1,
        opaque
    );
    assert_eq!(payload(&fields, "bonusChestEnabled"), (1, &[1][..]));
    assert_eq!(payload(&fields, "showcoordinates"), (1, &[1][..]));
    assert!(configured(&world, &new[..new.len() - 1]).is_none());
}

#[test]
fn world_options_reload_and_preserve_deep_unknown_compounds() {
    let mut world = test_support::world();
    world.options.set(O::ShowCoordinates, 1);
    world.options.set(O::AlwaysDay, 1);
    world.options.set(O::SleepSkipNight, 0);
    let bytes = configured(&world, &[]).unwrap();
    let dir = test_support::scratch();
    fs::write(dir.path().join("level.dat"), &bytes).unwrap();
    let loaded = read_options(&dir.path().join("level.dat"));
    assert!(loaded.enabled(O::ShowCoordinates));
    assert!(loaded.enabled(O::AlwaysDay));
    assert!(!loaded.enabled(O::SleepSkipNight));
    assert!(loaded.enabled(O::ActivateCheats));
    let mut body = bytes[8..bytes.len() - 1].to_vec();
    for _ in 0..128 {
        body.extend([10, 1, 0, b'x']);
    }
    body.extend([0; 129]);
    let mut deep = bytes[..4].to_vec();
    deep.extend((body.len() as u32).to_le_bytes());
    deep.extend(body);
    assert!(configured(&world, &deep).is_some());
    assert!(configured(&world, &deep[..deep.len() - 1]).is_none());
}

/// Replaces a fixture's clock while preserving its other level settings.
fn clock(bytes: &[u8], time: i64) -> Vec<u8> {
    let mut fields = read_fields(&bytes[8..]).unwrap();
    put(&mut fields, "Time", 4, &time.to_le_bytes());
    let mut body = vec![10, 0, 0];
    for (_, tag) in fields {
        body.extend(tag);
    }
    body.push(0);
    let mut bytes = bytes[..4].to_vec();
    bytes.extend((body.len() as u32).to_le_bytes());
    bytes.extend(body);
    bytes
}

#[test]
fn world_options_always_day_preserves_existing_days_and_saved_clock() {
    let mut world = test_support::world();
    world.options.set(O::AlwaysDay, 1);
    let initial = configured(&world, &[]).unwrap();
    let saved = clock(&initial, 246_100);
    let reopened = configured(&world, &saved).unwrap();
    assert_eq!(
        scalar(&read_fields(&reopened[8..]).unwrap(), "Time"),
        Some(246_100)
    );
    world.options.set(O::AlwaysDay, 0);
    world.options.set(O::DaylightCycle, 1);
    let cycling = configured(&world, &clock(&initial, 244_000)).unwrap();
    world.options.set(O::AlwaysDay, 1);
    let enabled = configured(&world, &cycling).unwrap();
    assert_eq!(
        scalar(&read_fields(&enabled[8..]).unwrap(), "Time"),
        Some(246_000)
    );
}

#[test]
fn world_options_player_permissions_preserve_the_separate_command_permission() {
    let world = test_support::world();
    let bytes = configured(&world, &[]).unwrap();
    let mut fields = read_fields(&bytes[8..]).unwrap();
    put(&mut fields, "permissionsLevel", 3, &1i32.to_le_bytes());
    put(
        &mut fields,
        "playerPermissionsLevel",
        3,
        &0i32.to_le_bytes(),
    );
    let mut body = vec![10, 0, 0];
    for (_, tag) in fields {
        body.extend(tag);
    }
    body.push(0);
    let mut fixture = bytes[..4].to_vec();
    fixture.extend((body.len() as u32).to_le_bytes());
    fixture.extend(body);
    let dir = test_support::scratch();
    fs::write(dir.path().join("level.dat"), &fixture).unwrap();
    assert_eq!(
        read_options(&dir.path().join("level.dat")).get(O::PlayerPermissions),
        0
    );
    let mut changed = world;
    changed.options.set(O::PlayerPermissions, 2);
    let updated = configured(&changed, &fixture).unwrap();
    let fields = read_fields(&updated[8..]).unwrap();
    assert_eq!(scalar(&fields, "permissionsLevel"), Some(1));
    assert_eq!(scalar(&fields, "playerPermissionsLevel"), Some(2));
}

/// A world BDS created before saved options existed keeps multiplayer although BDS records no intent.
#[test]
fn world_options_read_multiplayer_from_the_effective_flags_bds_saves() {
    let bytes = configured(&test_support::world(), &[]).unwrap();
    let mut fields = read_fields(&bytes[8..]).unwrap();
    put(&mut fields, "MultiplayerGame", 1, &[1]);
    put(&mut fields, "MultiplayerGameIntent", 1, &[0]);
    let mut body = vec![10, 0, 0];
    for (_, tag) in fields {
        body.extend(tag);
    }
    body.push(0);
    let mut fixture = bytes[..4].to_vec();
    fixture.extend((body.len() as u32).to_le_bytes());
    fixture.extend(body);
    let dir = test_support::scratch();
    fs::write(dir.path().join("level.dat"), &fixture).unwrap();
    let options = read_options(&dir.path().join("level.dat"));
    assert!(options.enabled(O::Multiplayer));
}

/// An oversized file is never parsed, and opening the world leaves it byte-identical.
#[test]
fn world_options_leave_level_data_beyond_the_read_bound_untouched() {
    let mut world = test_support::world();
    world.options.set(O::ShowCoordinates, 1);
    let bytes = configured(&world, &[]).unwrap();
    let oversized = test_support::padded_level_dat(&bytes, MAX_LEVEL_DAT_BYTES as usize);
    assert!(
        configured(&world, &oversized).is_some(),
        "the fixture is valid"
    );
    let dir = test_support::scratch();
    let path = dir.path().join("level.dat");
    fs::write(&path, &oversized).unwrap();
    assert!(try_read_options(&path).is_none());
    world.options.set(O::ShowCoordinates, 0);
    configure(&world, dir.path()).unwrap();
    assert!(fs::read(&path).unwrap() == oversized);
    fs::write(&path, &bytes).unwrap();
    assert!(try_read_options(&path).unwrap().enabled(O::ShowCoordinates));
}

/// New level data keeps LAN broadcast on, which BDS needs for the local offline join; existing
/// files keep whatever BDS saved.
#[test]
fn world_options_new_level_data_keeps_lan_broadcast_on() {
    let world = test_support::world();
    let bytes = configured(&world, &[]).unwrap();
    let fields = read_fields(&bytes[8..]).unwrap();
    assert_eq!(scalar(&fields, "LANBroadcast"), Some(1));
    assert_eq!(scalar(&fields, "LANBroadcastIntent"), Some(1));
    let mut saved = fields;
    put(&mut saved, "LANBroadcastIntent", 1, &[0]);
    let mut body = vec![10, 0, 0];
    for (_, tag) in saved {
        body.extend(tag);
    }
    body.push(0);
    let mut existing = bytes[..4].to_vec();
    existing.extend((body.len() as u32).to_le_bytes());
    existing.extend(body);
    let reopened = configured(&world, &existing).unwrap();
    let fields = read_fields(&reopened[8..]).unwrap();
    assert_eq!(scalar(&fields, "LANBroadcastIntent"), Some(0));
}
