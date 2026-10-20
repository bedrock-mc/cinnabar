use super::*;

use crate::{test_support::mini_engine_presentation, ui_runtime::UiRuntime};
use mod_ui::PanelUiOwner;
use serde_json::json;
use ui::{DpiScale, SafeArea, mod_hud::HudBindings, mod_panel::SurfaceValue};

/// Builds real inert HUD roots, including nested transforms, hidden and ambiguous names.
fn presentation() -> UiPresentationRuntime {
    let mut presentation = mini_engine_presentation();
    let panel = |name: &str, visible: bool| json!({name:{"type":"input_panel","size":[20,10],"visible":visible}});
    presentation.set_server_ui_pack(&super::super::super::super::ServerUiPack {
        ui_layers:vec![vec![
            ("ui/_ui_defs.json".into(),br#"{"ui_defs":["ui/hud_screen.json"]}"#.to_vec()),
            ("ui/hud_screen.json".into(),serde_json::to_vec(&json!({
                "namespace":"hud","hud_screen":{"type":"screen","controls":[
                    {"workspace":{"type":"panel","size":[500,300],"offset":[20,10],
                        "anchor_from":"top_left","anchor_to":"top_left",
                        "cinnabar_transform":{"scale":1.25,"origin":[0,0]},
                        "controls":[{"extension_hud_fixture":{"type":"input_panel",
                            "size":[80,27],"offset":[100,80],
                            "anchor_from":"top_left","anchor_to":"top_left",
                            "cinnabar_transform":{"scale":1.5,"origin":[0,0]},
                            "controls":[{"label":{"type":"label","text":"HUD","size":[80,27]}}]
                        }}]}},
                    {"first":{"type":"panel","controls":[panel("ambiguous",true)]}},
                    {"second":{"type":"panel","controls":[panel("ambiguous",true)]}},
                    panel("hidden",false),
                    {"mapped":{"type":"input_panel","size":[20,10],"button_mappings":[{
                        "from_button_id":"button.menu_select","to_button_id":"example.action","mapping_type":"pressed"
                    }]}}
                ]}
            })).unwrap())
        ]],..Default::default()
    });
    presentation
        .set_mod_hud_bindings(
            PanelUiOwner::PacketSession(1),
            Some(HudBindings(BTreeMap::from([(
                "#mod_geometry_enabled".into(),
                SurfaceValue::Bool(true),
            )]))),
        )
        .unwrap();
    presentation
}

/// Paints the actual HUD for one session without game assets or a network connection.
fn build(presentation: &mut UiPresentationRuntime, session: u64) {
    let mut player = player_state::PlayerState::new(1);
    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Survival);
    presentation
        .build(
            &player,
            &UiRuntime::new(session),
            0,
            [2560, 1600],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
}

#[test]
fn geometry_tracks_actual_root_bounds_across_hud_scales_and_excludes_safe_area_origin() {
    let requests = HudGeometryRequests(vec!["extension_hud_fixture".into()]);
    let mut reported = Vec::new();
    for preference in [1, 4] {
        let mut presentation = presentation();
        presentation.set_safe_area(SafeArea::new(20.0, 10.0, 40.0, 30.0).unwrap());
        presentation.set_gui_scale_preference(Some(preference));
        build(&mut presentation, 7);
        let passes = presentation.hud_passes();
        let geometry = presentation.mod_hud_geometry(&requests, 7);
        let root = &geometry["extension_hud_fixture"];
        let frame = presentation.screens.touch.hud.as_ref().unwrap();
        assert_eq!(frame.origin, [20.0, 10.0]);
        let unit = frame.scale / FONT_DESIGN_PIXEL_TEXELS as f32;
        assert_eq!(
            root.rect,
            [145.0, 110.0, 150.0, 50.625].map(|value| value * unit)
        );
        assert_eq!(root.size, [80.0, 27.0].map(|value| value * unit));
        assert_eq!(
            presentation.hud_passes(),
            passes,
            "reading geometry never lays out again"
        );
        reported.push(root.clone());
    }
    assert_ne!(
        reported[0], reported[1],
        "HUD GUI preferences must reach the panel's coordinate space"
    );
}

#[test]
fn hidden_missing_mapped_ambiguous_and_old_session_roots_are_not_reported() {
    let mut presentation = presentation();
    let requests = HudGeometryRequests(vec![
        "extension_hud_fixture".into(),
        "hidden".into(),
        "missing".into(),
        "mapped".into(),
        "ambiguous".into(),
    ]);
    assert!(
        presentation.mod_hud_geometry(&requests, 1).is_empty(),
        "there is no completed HUD frame"
    );
    build(&mut presentation, 1);
    let geometry = presentation.mod_hud_geometry(&requests, 1);
    assert_eq!(
        geometry.keys().map(String::as_str).collect::<Vec<_>>(),
        ["extension_hud_fixture"]
    );
    assert!(presentation.mod_hud_geometry(&requests, 2).is_empty());
    build(&mut presentation, 2);
    assert!(presentation.mod_hud_geometry(&requests, 1).is_empty());
    assert_eq!(presentation.mod_hud_geometry(&requests, 2).len(), 1);
    presentation.screens.touch.begin_frame();
    assert!(
        presentation.mod_hud_geometry(&requests, 2).is_empty(),
        "unpainted HUD geometry is retired with its frame"
    );
}

/// Keep-out input follows native pack layout and the editor's GUI scale, without a second layout.
#[test]
fn editor_keep_out_uses_the_completed_native_layout_and_rejects_old_sessions() {
    let mut p = crate::test_support::mini_engine_presentation();
    p.set_server_ui_pack(&super::super::super::super::ServerUiPack {
        ui_layers: vec![vec![
            ("ui/_ui_defs.json".into(), br#"{"ui_defs":["ui/hud_screen.json"]}"#.to_vec()),
            ("ui/hud_screen.json".into(), br#"{"namespace":"hud","hud_screen":{"type":"screen","controls":[{"hotbar_panel":{"type":"input_panel","size":[182,22],"offset":[30,200],"anchor_from":"top_left","anchor_to":"top_left","cinnabar_transform":{"scale":1.5,"origin":[0,0]}}}]}}"#.to_vec()),
        ]], ..Default::default()
    });
    p.mod_ui_mut()
        .set_mod_panel(
            PanelUiOwner::Environment(0),
            Some(&mod_ui::test_support::sample_panel()),
        )
        .unwrap();
    p.mod_ui_mut().set_mod_panel_open(true);
    p.mod_ui_mut()
        .open_mod_hud_editor(&ui::mod_hud::Hud::default())
        .unwrap();
    p.set_gui_scale_preference(Some(2));
    build(&mut p, 7);
    let frame = p.screens.touch.hud.as_ref().unwrap();
    let unit = f64::from(frame.scale / FONT_DESIGN_PIXEL_TEXELS as f32);
    let passes = p.hud_passes();
    assert_eq!(
        p.mod_hud_editor_native_keep_out(7),
        Some([30., 200., 273., 33.].map(|v| v * unit))
    );
    assert_eq!(p.hud_passes(), passes);
    assert_eq!(p.mod_hud_editor_native_keep_out(8), None);
    p.screens.touch.begin_frame();
    assert_eq!(p.mod_hud_editor_native_keep_out(7), None);
}
