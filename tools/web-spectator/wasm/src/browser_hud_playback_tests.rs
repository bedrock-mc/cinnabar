use json_ui::{
    Animated, Catalog, Context, HudModel, HudTitle, LayoutEnv, TextMeasure, TextureMeta,
    TextureSource, Timed, ViewState,
};

use super::BrowserHudPlayback;

const EVENT_TIME: f64 = 1_000.0;
const EVENT_TIMESTAMP: &str = "2026-01-01T00:00:00Z";
const VIEWPORT: [u32; 2] = [200, 200];
const LABELS: [&str; 2] = ["hud_actionbar_text", "hud_title_text"];

struct Text;
impl TextMeasure for Text {
    fn extent(&self, text: &str) -> [f64; 2] {
        [text.len() as f64 * 8.0, 8.0]
    }
}

struct Textures;
impl TextureSource for Textures {
    fn texture(&self, _path: &str) -> Option<TextureMeta> {
        None
    }
}

/// Builds an authored HUD fixture with factory-created, clocked labels that expire.
fn catalog() -> Catalog {
    let hud = br#"{
      "namespace": "hud",
      "hud_screen": {
        "type": "panel", "size": ["100%", "100%"],
        "controls": [
          {"actionbar@hud.actionbar_factory": {}},
          {"title@hud.title_factory": {}}
        ]
      },
      "actionbar_factory": {
        "type": "panel", "size": ["100%", "100%"],
        "factory": {
          "name": "hud_actionbar_text_factory",
          "control_ids": {"hud_actionbar_text": "hud_actionbar_text@hud.actionbar"}
        }
      },
      "title_factory": {
        "type": "panel", "size": ["100%", "100%"],
        "factory": {
          "name": "hud_title_text_factory",
          "control_ids": {"hud_title_text": "hud_title_text@hud.title"}
        }
      },
      "actionbar": {
        "type": "label", "size": [100, 10], "text": "$actionbar_text",
        "alpha": "@hud.actionbar_wait"
      },
      "title": {
        "type": "label", "size": [100, 10], "text": "$title_text",
        "alpha": "@hud.title_wait"
      },
      "actionbar_wait": {
        "anim_type": "wait", "duration": 2, "next": "@hud.actionbar_out"
      },
      "actionbar_out": {
        "anim_type": "alpha", "easing": "linear", "duration": 1,
        "from": 1, "to": 0, "destroy_at_end": "hud_actionbar_text"
      },
      "title_wait": {
        "anim_type": "wait", "duration": "$title_stay_time", "next": "@hud.title_out"
      },
      "title_out": {
        "anim_type": "alpha", "easing": "linear", "duration": "$title_fade_out_time",
        "from": 1, "to": 0, "destroy_at_end": "hud_title_text"
      }
    }"#;
    Catalog::from_files([
        ("ui/_global_variables.json", b"{}".as_slice()),
        (
            "ui/_ui_defs.json",
            br#"{"ui_defs":["ui/hud_screen.json"]}"#.as_slice(),
        ),
        ("ui/hud_screen.json", hud.as_slice()),
    ])
    .unwrap()
}

/// Samples the same recorded events while assigning their current title incarnation.
fn model(playback: &mut BrowserHudPlayback, generation: u64) -> HudModel {
    HudModel {
        actionbar: Some(Timed {
            text: "Recorded actionbar".into(),
            born: EVENT_TIME,
        }),
        title: Some(HudTitle {
            creation_id: playback
                .title_creation_id("fighter", Some(EVENT_TIMESTAMP), generation)
                .unwrap(),
            title: "Recorded title".into(),
            fade_in: 0.0,
            stay: 2.0,
            fade_out: 1.0,
            born: EVENT_TIME,
            ..HudTitle::default()
        }),
        ..HudModel::default()
    }
}

/// Installs a layout through the JSON-UI public rendering API.
fn install_layout(playback: &mut BrowserHudPlayback, catalog: &Catalog, model: HudModel) {
    let screen = json_ui::render_screen(
        json_ui::HUD_SCREEN,
        catalog,
        &Context::retail(false),
        &json_ui::hud_data_source(&model),
        VIEWPORT.map(f64::from),
        &LayoutEnv {
            text: &Text,
            textures: &Textures,
        },
        &ViewState::default(),
    )
    .unwrap();
    playback.cached = Some((model, VIEWPORT, screen.nodes));
}

/// Paints both timed labels with the retained animator and recorded event clocks.
fn frame(playback: &mut BrowserHudPlayback, now: f64) -> [Animated; 2] {
    let (model, _, nodes) = playback.cached.as_ref().unwrap();
    let clocks = json_ui::hud_clocks(model);
    let drawn = LABELS.map(|label| {
        nodes
            .iter()
            .find(|node| node.name == label)
            .unwrap()
            .animate(&mut playback.animator, now, Some(&clocks), None)
    });
    playback.animator.end_frame();
    drawn
}

#[test]
fn replay_reset_restores_faded_and_destroyed_labels_at_their_original_birth_time() {
    let catalog = catalog();
    for elapsed in [2.5, 4.0] {
        let mut playback = BrowserHudPlayback::default();
        playback.observe_fighter("fighter", 20.0, (EVENT_TIME * 1_000.0) as u64);
        let initial_model = model(&mut playback, 1);
        install_layout(&mut playback, &catalog, initial_model.clone());
        for label in frame(&mut playback, EVENT_TIME) {
            assert_eq!(label.opacity, 1.0);
            assert!(!label.hidden);
        }
        frame(&mut playback, EVENT_TIME + elapsed);
        // The next paint observes any destruction issued by the prior animation tick.
        for label in frame(&mut playback, EVENT_TIME + elapsed) {
            if elapsed < 3.0 {
                assert_eq!(label.opacity, 0.5);
                assert!(!label.hidden);
            } else {
                assert!(label.hidden);
            }
        }
        playback.observe_fighter("fighter", 10.0, ((EVENT_TIME + elapsed) * 1_000.0) as u64);
        assert!(playback.last_health_drop_millis.is_some());
        for label in frame(&mut playback, EVENT_TIME + 1.0) {
            assert!(label.hidden || label.opacity < 1.0);
        }

        playback.reset();
        assert!(playback.cached.is_none());
        playback.observe_fighter("fighter", 5.0, ((EVENT_TIME + 1.0) * 1_000.0) as u64);
        assert_eq!(playback.last_health_drop_millis, None);
        let rewound_model = model(&mut playback, 2);
        assert_ne!(
            rewound_model.title.as_ref().unwrap().creation_id,
            initial_model.title.as_ref().unwrap().creation_id
        );
        assert_eq!(
            json_ui::hud_clocks(&rewound_model),
            json_ui::hud_clocks(&initial_model)
        );
        install_layout(&mut playback, &catalog, rewound_model);
        for label in frame(&mut playback, EVENT_TIME + 1.0) {
            assert_eq!(label.opacity, 1.0);
            assert!(!label.hidden);
        }
    }
}

#[test]
fn title_incarnation_is_stable_until_the_pov_disappears() {
    let mut playback = BrowserHudPlayback::default();
    playback.observe_fighter("fighter", 20.0, 1_000);
    let first = playback.title_creation_id("fighter", Some(EVENT_TIMESTAMP), 1);
    assert_eq!(
        playback.title_creation_id("fighter", Some(EVENT_TIMESTAMP), 2),
        first
    );
    playback.observe_fighter("fighter", 10.0, 1_100);
    assert_eq!(playback.last_health_drop_millis, Some(1_100));
    playback.clear_fighter();
    playback.observe_fighter("fighter", 5.0, 1_200);
    assert_eq!(playback.last_health_drop_millis, None);
    assert_ne!(
        playback.title_creation_id("fighter", Some(EVENT_TIMESTAMP), 3),
        first
    );
    assert_eq!(playback.title_creation_id("fighter", None, 4), None);
}
