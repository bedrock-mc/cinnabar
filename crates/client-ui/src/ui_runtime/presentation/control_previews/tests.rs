use super::*;
use crate::{test_support::mini_engine_presentation, ui_runtime::UiRuntime};
use ui::{
    DpiScale, IconRef,
    mod_panel::{Control, Panel, Surface, SurfaceTransition, SurfaceValue},
};

const OWNER: PanelUiOwner = PanelUiOwner::PacketSession(7);

fn pack() -> crate::ui_runtime::presentation::ServerUiPack {
    crate::ui_runtime::presentation::ServerUiPack {
        ui_layers:vec![vec![
            ("ui/_ui_defs.json".into(),br#"{"ui_defs":["ui/preview_fixture.json","ui/hud_screen.json"]}"#.to_vec()),
            ("ui/preview_fixture.json".into(),json!({"namespace":"preview_fixture","actual":{
                "type":"input_panel","$tint":[1,0,0,1],"size":[48,28],"visible":false,
                "bindings":[{"binding_name":"#fixture_shown","binding_name_override":"#visible"}],
                "controls":[{"pixels":{"type":"custom","renderer":"cinnabar_rounded_rectangle","size":[48,28],"radius":1,"color":"$tint"}},
                    {"inner":{"type":"button","size":[48,28],"button_mappings":[{"from_button_id":"button.menu_select","to_button_id":"unrelated.inner","mapping_type":"pressed"}]}}]},
                "player":{"type":"input_panel","size":[15,15],"controls":[{"actor":{"type":"custom","renderer":"hud_player_renderer","size":[15,15]}}]},
                "overflow":{"type":"panel","size":[20,10],"controls":[{"pixels":{"type":"custom","renderer":"cinnabar_rounded_rectangle","offset":[-12,-6],"size":[48,28],"radius":1,"color":[0,1,0,1]}}]}
            }).to_string().into_bytes()),
            ("ui/hud_screen.json".into(),json!({"namespace":"hud","hud_screen":{"type":"screen","controls":[{"actual@preview_fixture.actual":{}}]}}).to_string().into_bytes()),
        ]],..Default::default()
    }
}

fn panel(reference: &str) -> Panel {
    let mut panel = mod_ui::test_support::sample_panel();
    panel.motion = true;
    panel.reference_size = None;
    panel.controls = vec![Control::Button {
        id: "outer".into(),
        label: "Outer action".into(),
    }];
    panel.surface=Some(Surface {
        collections: Default::default(),
        screen:"preview_menu.root".into(),
        document:json!({"namespace":"preview_menu","root":{"type":"screen","controls":[{"card":{"type":"button","offset":[20,20],"size":[100,50],
            "button_mappings":[{"from_button_id":"button.menu_select","to_button_id":"mod.control:0","mapping_type":"pressed"}],
            "controls":[{"thumbnail":{"type":"custom","renderer":RENDERER,"preview_id":"example","size":["100%","100%"]}}]}}]}}).to_string(),
        previews:BTreeMap::from([("example".into(),ControlPreview {
            reference:reference.into(),
            variables:BTreeMap::from([("$tint".into(),json!([0,1,0,1]))]),
            globals:BTreeMap::from([("#fixture_shown".into(),json!(true))]),
            ..Default::default()
        })]),
        bindings:BTreeMap::from([("#opacity".into(),SurfaceValue::Number(1.0))]),
        transitions:BTreeMap::from([("#opacity".into(),SurfaceTransition {
            duration:0.2, easing:Default::default(), from:None, exit:Some(SurfaceValue::Number(0.0))
        })]),
    });
    panel
}

fn build(presentation: &mut UiPresentationRuntime, millis: u64) -> render_model::UiRenderInput {
    presentation
        .build(
            &player_state::PlayerState::new(1),
            &UiRuntime::new(1),
            millis,
            [1280, 800],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap()
}

/// Coverage artwork may add a sub-GUI-pixel antialias fringe to its authored extent.
fn assert_extent(actual: [f32; 2], authored: [f32; 2]) {
    for (actual, authored) in actual.into_iter().zip(authored) {
        assert!(
            actual >= authored && actual < authored + 1.0,
            "{actual} vs {authored}"
        );
    }
}

#[test]
fn control_preview_uses_real_pack_pixels_isolated_variables_and_only_outer_input() {
    let mut presentation = mini_engine_presentation();
    presentation.set_gui_scale_preference(Some(2));
    presentation.set_server_ui_pack(&pack());
    presentation
        .mod_ui_mut()
        .set_mod_panel(OWNER, Some(&panel("preview_fixture.actual")))
        .unwrap();
    presentation.mod_ui_mut().set_mod_panel_open(true);
    let frame = build(&mut presentation, 0);
    assert!(
        frame
            .vertices
            .iter()
            .any(|vertex| vertex.color[1] > 200 && vertex.color[0] < 20)
    );
    assert_extent(
        presentation.mod_control_preview_sizes(OWNER, 1)["example"],
        [48.0, 28.0],
    );
    assert!(
        !presentation
            .hud_draw_nodes()
            .iter()
            .any(|node| node.name == "pixels")
    );
    let entry = &presentation.control_previews.entries["example"];
    let passes = entry.screen.passes;
    build(&mut presentation, 1);
    assert_eq!(
        presentation.control_previews.entries["example"]
            .screen
            .passes,
        passes
    );
    let state = presentation.mod_ui().mod_panel_state().unwrap();
    let drawn = state.frame.unwrap();
    assert!(
        drawn
            .hits
            .iter()
            .all(|hit| hit.pressed.as_deref() != Some("unrelated.inner"))
    );
    let hit = drawn
        .hits
        .iter()
        .find(|hit| hit.pressed.as_deref() == Some("mod.control:0"))
        .unwrap();
    let position = [
        (hit.rect.x + hit.rect.w / 2.0) as f32 * drawn.scale,
        (hit.rect.y + hit.rect.h / 2.0) as f32 * drawn.scale,
    ];
    assert_eq!(
        presentation
            .mod_ui_mut()
            .mod_panel_events(position, true, true)[0]
            .id,
        "outer"
    );
    presentation.set_gui_scale_preference(Some(3));
    build(&mut presentation, 2);
    assert_extent(
        presentation.mod_control_preview_sizes(OWNER, 1)["example"],
        [72.0, 42.0],
    );
}

#[test]
fn control_preview_measurements_never_cross_owner_session_or_declaration_boundaries() {
    let mut presentation = mini_engine_presentation();
    presentation.set_server_ui_pack(&pack());
    let initial = panel("preview_fixture.actual");
    presentation
        .mod_ui_mut()
        .set_mod_panel(OWNER, Some(&initial))
        .unwrap();
    presentation.mod_ui_mut().set_mod_panel_open(true);
    build(&mut presentation, 0);
    assert!(!presentation.mod_control_preview_sizes(OWNER, 1).is_empty());
    assert!(presentation.mod_control_preview_sizes(OWNER, 2).is_empty());
    assert!(
        presentation
            .mod_control_preview_sizes(PanelUiOwner::Registration(7), 1)
            .is_empty()
    );
    let missing = panel("preview_fixture.missing");
    presentation
        .mod_ui_mut()
        .set_mod_panel(OWNER, Some(&missing))
        .unwrap();
    assert!(presentation.mod_control_preview_sizes(OWNER, 1).is_empty());
    build(&mut presentation, 1);
    assert!(presentation.mod_control_preview_sizes(OWNER, 1).is_empty());
    assert!(
        presentation.control_previews.entries["example"]
            .nodes
            .is_empty()
    );
    build(&mut presentation, 2);
    assert!(presentation.mod_control_preview_sizes(OWNER, 1).is_empty());
    presentation
        .mod_ui_mut()
        .set_mod_panel(PanelUiOwner::Registration(9), Some(&initial))
        .unwrap();
    build(&mut presentation, 3);
    assert!(presentation.mod_control_preview_sizes(OWNER, 1).is_empty());
}

#[test]
fn control_preview_player_demand_keeps_hidden_doll_settings_and_survives_only_its_exit() {
    let mut presentation = mini_engine_presentation();
    presentation.set_server_ui_pack(&pack());
    assert!(!presentation.hud_frame.paper_doll_visible);
    presentation.hud_frame.player_preview = Some(IconRef {
        page: presentation.solid_texture_page,
        uv: [0, 0, u16::MAX, u16::MAX],
        glint: false,
    });
    presentation
        .mod_ui_mut()
        .set_mod_panel(OWNER, Some(&panel("preview_fixture.player")))
        .unwrap();
    presentation.mod_ui_mut().set_mod_panel_open(true);
    build(&mut presentation, 0);
    assert!(presentation.control_preview_needs_player(1));
    assert!(!presentation.control_preview_needs_player(2));
    assert!(!presentation.hud_frame.paper_doll_visible);
    assert_eq!(
        presentation.player_preview_view,
        super::super::player_preview::PreviewView::Hud
    );
    presentation.mod_ui_mut().set_mod_panel_open(false);
    assert!(presentation.control_preview_needs_player(1));
    build(&mut presentation, 1);
    assert!(presentation.control_preview_needs_player(1));
    build(&mut presentation, 500);
    assert!(!presentation.control_preview_needs_player(1));
    assert!(presentation.mod_control_preview_sizes(OWNER, 1).is_empty());
    assert!(!presentation.hud_frame.paper_doll_visible);
}

#[test]
fn control_preview_demand_and_measurements_yield_to_the_builtin_hud_editor() {
    let mut presentation = mini_engine_presentation();
    presentation.set_server_ui_pack(&pack());
    presentation.hud_frame.player_preview = Some(IconRef {
        page: presentation.solid_texture_page,
        uv: [0, 0, u16::MAX, u16::MAX],
        glint: false,
    });
    presentation
        .mod_ui_mut()
        .set_mod_panel(OWNER, Some(&panel("preview_fixture.player")))
        .unwrap();
    presentation.mod_ui_mut().set_mod_panel_open(true);
    build(&mut presentation, 0);
    assert!(presentation.control_preview_needs_player(1));
    assert!(!presentation.mod_control_preview_sizes(OWNER, 1).is_empty());
    presentation
        .mod_ui_mut()
        .open_mod_hud_editor(&ui::mod_hud::Hud::default())
        .unwrap();
    assert!(!presentation.control_preview_needs_player(1));
    assert!(presentation.mod_control_preview_sizes(OWNER, 1).is_empty());
}

#[test]
fn control_preview_contains_artwork_outside_the_authored_reference_canvas() {
    let mut presentation = mini_engine_presentation();
    presentation.set_gui_scale_preference(Some(2));
    presentation.set_server_ui_pack(&pack());
    let mut panel = panel("preview_fixture.overflow");
    panel
        .surface
        .as_mut()
        .unwrap()
        .previews
        .get_mut("example")
        .unwrap()
        .size = [20.0, 10.0];
    presentation
        .mod_ui_mut()
        .set_mod_panel(OWNER, Some(&panel))
        .unwrap();
    presentation.mod_ui_mut().set_mod_panel_open(true);
    let frame = build(&mut presentation, 0);
    assert_extent(
        presentation.mod_control_preview_sizes(OWNER, 1)["example"],
        [48.0, 28.0],
    );
    assert!(
        frame
            .vertices
            .iter()
            .any(|vertex| vertex.color[1] > 200 && vertex.color[0] < 20)
    );
}
