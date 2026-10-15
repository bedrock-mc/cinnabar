//! Retained bindings keep paint clocks independent from semantic construction.

use super::*;
use json_ui::{HudChanges, HudTitle, Timed, hud_clocks, hud_data_source};

#[test]
fn idle_frames_share_data_and_crosshair_without_freezing_scene_clocks() {
    let mut screens = HudScreens::default();
    let catalog = Arc::new(Catalog::default());
    let model = HudModel {
        title: Some(HudTitle {
            creation_id: 7,
            born: 1.0,
            ..Default::default()
        }),
        actionbar: Some(Timed {
            text: "§kChanging glyphs".into(),
            born: 2.0,
        }),
        ..Default::default()
    };
    let scene = BTreeMap::from([("screen_open".to_owned(), 3.0)]);
    let before = screens.begin(Some((&model, HudChanges::ALL, 70)), &scene, 1, &catalog);
    for clock in [4.0, 8.0] {
        let scene = BTreeMap::from([("screen_open".to_owned(), clock)]);
        let after = screens.begin(
            Some((&model, HudChanges::default(), 70)),
            &scene,
            1,
            &catalog,
        );
        assert!(Arc::ptr_eq(&before, &after));
        assert_eq!(screens.clocks["screen_open"], clock);
        for (key, born) in hud_clocks(&model) {
            assert_eq!(screens.clocks[&key], born);
        }
    }
    let first = screens.begin(None, &scene, 1, &catalog);
    let next = screens.begin(None, &scene, 2, &Arc::new(Catalog::default()));
    assert!(Arc::ptr_eq(&first, &next));
    assert_eq!(*first, DataSource::default());
    let after = screens.begin(
        Some((&model, HudChanges::default(), 70)),
        &BTreeMap::new(),
        1,
        &catalog,
    );
    assert!(Arc::ptr_eq(&before, &after));
    assert!(!screens.clocks.contains_key("screen_open"));
}

#[test]
fn opacity_status_and_text_updates_preserve_controller_answers() {
    let mut screens = HudScreens::default();
    let catalog = Arc::new(Catalog::default());
    let mut model = HudModel::default();
    let clocks = BTreeMap::new();
    for (changes, opacity) in [
        (HudChanges::ALL, 100),
        (HudChanges::default(), 30),
        (
            HudChanges {
                status: true,
                ..Default::default()
            },
            30,
        ),
        (
            HudChanges {
                text: true,
                ..Default::default()
            },
            30,
        ),
    ] {
        if changes.status {
            model.level += 1;
        }
        if changes.text {
            model.title = Some(HudTitle {
                creation_id: u64::from(model.level),
                born: f64::from(model.level),
                ..Default::default()
            });
        }
        let data = screens.begin(Some((&model, changes, opacity)), &clocks, 1, &catalog);
        let mut expected = hud_data_source(&model);
        expected.set_global(
            "#hud_alpha",
            json_ui::Scalar::Num(f64::from(opacity) / 100.0),
        );
        expected.set_global("#hud_propagate_alpha", json_ui::Scalar::Bool(true));
        assert_eq!(*data, expected);
        assert_eq!(screens.model_clocks, hud_clocks(&model));
    }
}
