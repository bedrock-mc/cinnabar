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
                Control::Number { .. } => {
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
