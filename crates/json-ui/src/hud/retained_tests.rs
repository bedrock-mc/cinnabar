//! Component updates preserve the controller's complete binding contract.

use super::*;
use std::sync::Arc;

#[test]
fn independent_updates_match_fresh_bindings_and_remove_retired_inputs() {
    let mut model = HudModel::default();
    let mut bindings = HudBindings::default();
    let initial = bindings.update(&model, HudChanges::ALL);
    assert_eq!(*initial, hud_data_source(&model));
    assert!(Arc::ptr_eq(
        &initial,
        &bindings.update(&model, HudChanges::default())
    ));
    for component in 0..HudChanges::ALL.components().len() {
        let changes = match component {
            0 => {
                model.survival_ui = true;
                model.xp_bar = true;
                model.level = 8;
                HudChanges {
                    status: true,
                    ..Default::default()
                }
            }
            1 => {
                model.hotbar = vec![HudSlot {
                    icon: Some(3),
                    count: 64,
                    selected: true,
                    durability: Some(0.5),
                    ..Default::default()
                }];
                model.offhand = Some(HudSlot::default());
                HudChanges {
                    hotbar: true,
                    ..Default::default()
                }
            }
            2 => {
                model.title = Some(HudTitle {
                    creation_id: 8,
                    title: "Title".into(),
                    subtitle: "Sub".into(),
                    born: 1.0,
                    ..Default::default()
                });
                model.actionbar = Some(Timed {
                    text: "Bar".into(),
                    born: 2.0,
                });
                model.tip = model.actionbar.clone();
                model.item_name = model.actionbar.clone();
                HudChanges {
                    text: true,
                    ..Default::default()
                }
            }
            3 => {
                model.chat_visible = true;
                model.chat = vec![Timed {
                    text: "§kChat".into(),
                    born: 3.0,
                }];
                model.chat_lifetime = 10.0;
                HudChanges {
                    chat: true,
                    ..Default::default()
                }
            }
            4 => {
                model.sidebar = Some(Sidebar {
                    title: "Score".into(),
                    rows: vec![("Player".into(), "8".into())],
                    ..Default::default()
                });
                HudChanges {
                    sidebar: true,
                    ..Default::default()
                }
            }
            5 => {
                model.boss_bars = vec![BossBar {
                    name: "Boss".into(),
                    progress: 0.5,
                    color: "#ff0000".into(),
                    notches: 6,
                }];
                HudChanges {
                    boss_bars: true,
                    ..Default::default()
                }
            }
            6 => {
                model.player_position = Some("Position".into());
                model.days_played = Some("Days".into());
                model.text_background_alpha = 0.5;
                HudChanges {
                    world_text: true,
                    ..Default::default()
                }
            }
            _ => unreachable!(),
        };
        assert_eq!(*bindings.update(&model, changes), hud_data_source(&model));
    }
    model.title = None;
    model.actionbar = None;
    model.tip = None;
    model.item_name = None;
    model.chat_visible = false;
    model.sidebar = None;
    model.boss_bars.clear();
    model.player_position = None;
    model.days_played = None;
    assert_eq!(
        *bindings.update(
            &model,
            HudChanges {
                text: true,
                chat: true,
                sidebar: true,
                boss_bars: true,
                world_text: true,
                ..Default::default()
            }
        ),
        hud_data_source(&model)
    );
    assert_eq!(
        *initial,
        hud_data_source(&HudModel::default()),
        "old publications remain immutable"
    );
}

#[test]
fn status_updates_share_unaffected_collections_and_idle_data() {
    let mut model = HudModel {
        hotbar: vec![HudSlot::default(); 9],
        chat_visible: true,
        chat: vec![Timed {
            text: "Chat".into(),
            born: 0.0,
        }],
        ..Default::default()
    };
    let mut bindings = HudBindings::default();
    let before = bindings.update(&model, HudChanges::ALL);
    model.level = 12;
    let after = bindings.update(
        &model,
        HudChanges {
            status: true,
            ..Default::default()
        },
    );
    for name in ["hotbar_items", "chat_text_grid", "boss_bars"] {
        assert_eq!(
            before.collections[name].as_ptr(),
            after.collections[name].as_ptr()
        );
    }
    assert!(Arc::ptr_eq(
        &before.factories["chat_item_factory"],
        &after.factories["chat_item_factory"]
    ));
    let same = bindings.update(&model, HudChanges::default());
    assert!(Arc::ptr_eq(&after, &same));
}

#[test]
fn component_binding_names_are_disjoint() {
    let model = HudModel {
        title: Some(HudTitle::default()),
        actionbar: Some(Timed::default()),
        tip: Some(Timed::default()),
        item_name: Some(Timed::default()),
        chat_visible: true,
        sidebar: Some(Sidebar::default()),
        player_position: Some(String::new()),
        days_played: Some(String::new()),
        ..Default::default()
    };
    let mut names = std::collections::BTreeSet::new();
    for write in HUD_PARTS {
        let mut part = DataSource::new();
        write(&mut part, &model);
        for (kind, keys) in [
            ("global", part.globals.keys().collect::<Vec<_>>()),
            ("collection", part.collections.keys().collect()),
            ("defaults", part.collection_defaults.keys().collect()),
            ("factory", part.factories.keys().collect()),
        ] {
            for key in keys {
                assert!(
                    names.insert((kind, key.clone())),
                    "shared component binding {key}"
                );
            }
        }
    }
}
