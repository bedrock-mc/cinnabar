use super::{FloatRange, SoundEventTables};
use serde_json::json;

#[test]
fn dead_bush_inherits_grass_interactions_and_keeps_its_ambient_route() {
    let tables = SoundEventTables::from_json(
        &json!({"block_sounds": {
            "deadbush": {"base": "grass", "events": {"ambient": "bush.rustle"}},
            "grass": {"events": {
                "break": {"sound": "dig.grass", "volume": 0.7, "pitch": [0.8, 1.0]},
                "hit": "hit.grass", "place": "place.grass", "default": ""
            }}
        }}),
        &json!({"deadbush": "deadbush"}),
    );
    let material = tables.material_of("minecraft:deadbush").unwrap();
    let broken = tables
        .block(material, "break")
        .expect("inherited break sound");
    assert_eq!(&*broken.sound, "dig.grass");
    assert_eq!(broken.volume, FloatRange { min: 0.7, max: 0.7 });
    assert_eq!(broken.pitch, FloatRange { min: 0.8, max: 1.0 });
    assert_eq!(&*tables.block(material, "hit").unwrap().sound, "hit.grass");
    assert_eq!(
        &*tables.block(material, "place").unwrap().sound,
        "place.grass"
    );
    assert_eq!(
        &*tables.block(material, "ambient").unwrap().sound,
        "bush.rustle"
    );
    assert!(tables.block(material, "unknown").is_none());
}

#[test]
fn inherited_events_keep_their_dynamics_and_explicit_overrides_win() {
    let tables = SoundEventTables::from_json(
        &json!({"block_sounds": {
            "child": {"base": "middle", "volume": 0.25, "pitch": 2.0, "events": {
                "ambient": "child.ambient", "hit": "", "default": ""
            }},
            "middle": {"base": "parent", "events": {"place": "middle.place"}},
            "parent": {"volume": 0.5, "pitch": [0.8, 1.0], "events": {
                "break": {"sound": "parent.break", "volume": 0.8},
                "hit": "parent.hit", "place": "parent.place", "default": "parent.default"
            }}
        }}),
        &json!({}),
    );
    let broken = tables.block("child", "break").unwrap();
    assert_eq!(&*broken.sound, "parent.break");
    assert_eq!(broken.volume, FloatRange { min: 0.8, max: 0.8 });
    assert_eq!(broken.pitch, FloatRange { min: 0.8, max: 1.0 });
    let ambient = tables.block("child", "ambient").unwrap();
    assert_eq!(
        ambient.volume,
        FloatRange {
            min: 0.25,
            max: 0.25
        }
    );
    assert_eq!(ambient.pitch, FloatRange { min: 2.0, max: 2.0 });
    assert_eq!(
        &*tables.block("child", "place").unwrap().sound,
        "middle.place"
    );
    assert!(tables.block("child", "hit").is_none());
    assert!(tables.block("child", "unknown").is_none());
}

#[test]
fn missing_and_recursive_bases_preserve_local_events_and_terminate() {
    let tables = SoundEventTables::from_json(
        &json!({"block_sounds": {
            "missing": {"base": "absent", "events": {"break": "local.break"}},
            "self": {"base": "self", "events": {"break": "self.break"}},
            "a": {"base": "b", "events": {"hit": "a.hit"}},
            "b": {"base": "a", "events": {"break": "b.break"}}
        }}),
        &json!({}),
    );
    for (material, sound) in [
        ("missing", "local.break"),
        ("self", "self.break"),
        ("a", "b.break"),
    ] {
        assert_eq!(&*tables.block(material, "break").unwrap().sound, sound);
        assert!(tables.block(material, "unknown").is_none());
    }
}

#[test]
fn inherited_routes_follow_pack_overrides_of_the_base() {
    let mut tables = SoundEventTables::from_json(
        &json!({"block_sounds": {
            "child": {"base": "parent", "events": {"ambient": "child.ambient"}},
            "parent": {"events": {"break": "parent.break"}}
        }}),
        &json!({}),
    );
    tables.merge(SoundEventTables::from_json(
        &json!({"block_sounds": {"parent": {"events": {"break": "pack.break"}}}}),
        &json!({}),
    ));
    assert_eq!(
        &*tables.block("child", "break").unwrap().sound,
        "pack.break"
    );
    assert_eq!(
        &*tables.block("child", "ambient").unwrap().sound,
        "child.ambient"
    );
}
