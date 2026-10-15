//! Cached screens follow their text environment and resolution context.

use super::*;
use json_ui::{Context, DataSource, ViewState};
use std::borrow::Cow;
use ui_engine::cached_screen::CachedScreen;

struct Metrics(&'static str);

impl json_ui::TextMeasure for Metrics {
    fn extent(&self, text: &str) -> [f64; 2] {
        [text.len() as f64 * 6.0, 9.0]
    }

    fn localize<'a>(&self, text: &'a str) -> Cow<'a, str> {
        Cow::Borrowed(if text == "label.key" { self.0 } else { text })
    }
}

impl json_ui::TextureSource for Metrics {
    fn texture(&self, _: &str) -> Option<json_ui::TextureMeta> {
        None
    }
}

#[test]
fn cached_hud_remeasures_language_and_context_changes() {
    let catalog = Arc::new(
        Catalog::from_files([
            ("ui/_global_variables.json", b"{}".as_slice()),
            ("ui/_ui_defs.json", br#"{"ui_defs":["ui/cache.json"]}"#.as_slice()),
            (
            "ui/cache.json",
            br#"{"namespace":"cache","root":{"type":"label","text":"$label","size":["default","default"]}}"#.as_slice(),
        )])
        .unwrap(),
    );
    let mut context = Context::desktop().with_var("label", serde_json::json!("label.key"));
    let mut cache = CachedScreen::default();
    let mut data = DataSource::new();
    let state = ViewState::default();
    let metrics = Metrics("unused");
    assert!(
        cache
            .render_with(
                "cache.missing",
                &catalog,
                &context,
                data.clone(),
                ([480.0, 270.0], 1.0, [0; 3]),
                &json_ui::LayoutEnv {
                    text: &metrics,
                    textures: &metrics
                },
                &state,
            )
            .is_none()
    );
    for (revision, translated, data_change, context_change) in [
        (0, "one", false, false),
        (1, "longer translation", false, false),
        (2, "another longer translation", true, false),
        (2, "another longer translation", false, true),
    ] {
        if data_change {
            data.set_global("#unrelated", json_ui::Scalar::Bool(true));
        }
        if context_change {
            context = context.with_var("label", serde_json::json!("context changed"));
        }
        let metrics = Metrics(translated);
        let env = json_ui::LayoutEnv {
            text: &metrics,
            textures: &metrics,
        };
        let render = cache
            .render_with(
                "cache.root",
                &catalog,
                &context,
                data.clone(),
                ([480.0, 270.0], 1.0, [revision, 0, 0]),
                &env,
                &state,
            )
            .unwrap();
        let expected = if context_change {
            "context changed"
        } else {
            translated
        };
        assert_eq!(render.nodes[0].dest.w, expected.len() as f64 * 6.0);
    }
    assert_eq!(cache.passes, 4);
}

// The pack worker resolves the HUD and crosshair, so the frame installing a server catalog binds
// them without resolving; an unprepared catalog still resolves on its first frame.
#[test]
fn prepared_server_catalogs_resolve_no_hud_on_the_frame() {
    for prepared in [true, false] {
        let Some(mut presentation) = crate::test_support::engine_presentation() else {
            eprintln!(
                "skipping prepared_server_catalogs_resolve_no_hud_on_the_frame: missing local UI carrier (make assets)"
            );
            return;
        };
        let base = presentation.pack_catalog_base().unwrap();
        let pack = if prepared {
            (*super::super::ServerUiPack::default().prepare_catalog(&base)).clone()
        } else {
            super::super::ServerUiPack {
                catalog: Some(Arc::new(
                    super::super::screen_registry::registry()
                        .catalog()
                        .layer(&base, &[])
                        .pack,
                )),
                ..super::super::ServerUiPack::default()
            }
        };
        presentation.set_server_ui_pack(&pack);
        presentation.hud_frame_mut().first_person = true;
        // A survival player in a session shows the HUD.
        let mut player = player_state::PlayerState::new(1);
        player
            .facts
            .publish_player_game_mode(protocol::PlayerGameMode::Survival);
        presentation
            .build(
                &player,
                &UiRuntime::new(1),
                0,
                [800, 600],
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        let screens = &presentation.screens.hud;
        assert!(screens.hud.passes > 0, "the HUD bound");
        let resolves = screens.hud.resolves + screens.crosshair.resolves;
        assert_eq!(
            resolves,
            if prepared { 0 } else { 2 },
            "prepared={prepared} resolved {resolves} screens on the frame"
        );
    }
}

#[test]
fn retained_semantics_keep_hotbar_hits_during_idle_and_text_updates() {
    let Some(mut presentation) = crate::test_support::engine_presentation() else {
        eprintln!(
            "skipping retained_semantics_keep_hotbar_hits_during_idle_and_text_updates: missing local UI carrier (make assets)"
        );
        return;
    };
    let mut player = player_state::PlayerState::new(1);
    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Survival);
    player.inventory.set_local_selected_slot(0);
    let mut runtime = UiRuntime::new(1);
    presentation.hud_frame_mut().first_person = true;
    presentation
        .build(
            &player,
            &runtime,
            0,
            [800, 600],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    let frame = presentation.screens.touch.hud.as_ref().unwrap();
    let hit = frame
        .hits
        .iter()
        .find(|hit| hit.collection.as_deref() == Some("hotbar_items"))
        .expect("the hotbar has touch targets");
    let point = [hit.rect.x + hit.rect.w * 0.5, hit.rect.y + hit.rect.h * 0.5];
    let slot = hit.collection_index;
    let before = presentation.hud_component_constructions();
    for now in [1, 2, 3] {
        if now == 3 {
            runtime.hud.set_actionbar(Arc::from("Updated"), 1, now);
        }
        presentation
            .build(
                &player,
                &runtime,
                now,
                [800, 600],
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        let frame = presentation.screens.touch.hud.as_ref().unwrap();
        let hit =
            json_ui::hit_test(&frame.hits, point).expect("the same point selects a hotbar slot");
        assert_eq!(hit.collection.as_deref(), Some("hotbar_items"));
        assert_eq!(hit.collection_index, slot);
        let after = presentation.hud_component_constructions();
        let expected = json_ui::HudChanges {
            text: now == 3,
            ..Default::default()
        }
        .components();
        for ((before, after), changed) in before.into_iter().zip(after).zip(expected) {
            assert_eq!(after - before, usize::from(changed));
        }
    }
}
