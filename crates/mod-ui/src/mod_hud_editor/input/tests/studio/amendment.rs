//! Placement bindings and independent editor preferences follow the public contract.

use super::*;

/// Opens a guest-authored payload through validation and the public editor entry point.
fn published(value: serde_json::Value) -> HudEditor {
    let hud: Hud = serde_json::from_value(value).unwrap();
    let mut ui = ModUi::default();
    ui.set_mod_panel(
        crate::PanelUiOwner::Environment(0),
        Some(&crate::test_support::sample_panel()),
    )
    .unwrap();
    ui.set_mod_panel_open(true);
    ui.open_mod_hud_editor(&hud).unwrap();
    let mut editor = ui.mod_hud_editor.take().unwrap();
    editor.viewport = [900., 600.];
    editor
}

#[test]
fn amendment_snap_and_grid_toggle_independently_and_shift_preserves_both() {
    let mut editor = editor(false);
    for (action, grid, snap) in [
        ("hud.grid", true, false),
        ("hud.snap", true, true),
        ("hud.grid", false, true),
        ("hud.snap", false, false),
    ] {
        press(&mut editor, action, &mut []);
        let data = editor.data();
        assert_eq!(data.global_value("#hud_grid_on"), Some(&Scalar::Bool(grid)));
        assert_eq!(
            data.global_value("#grid_visible"),
            Some(&Scalar::Bool(grid))
        );
        assert_eq!(data.global_value("#hud_snap_on"), Some(&Scalar::Bool(snap)));
        editor.bypass_snap = true;
        assert_eq!(editor.data(), data);
        editor.bypass_snap = false;
    }
}

#[test]
fn amendment_initial_hidden_cards_stay_listed_and_show_is_undoable() {
    let mut editor = published(serde_json::json!({"cards":[{
        "id":"disabled","title":"Disabled module","visible":false,"rows":[]
    }]}));
    let data = editor.data();
    assert_eq!(
        data.global_value("#hud_card_0_visible"),
        Some(&Scalar::Bool(false))
    );
    assert_eq!(
        data.global_value("#hud_card_0_name"),
        Some(&Scalar::Text("disabled".into()))
    );
    assert_eq!(editor.snapshot(true).hidden_cards, ["disabled"]);
    let catalog = template::catalog(&editor.draft, editor.viewport).unwrap();
    let root = json_ui::resolve(&catalog, SCREEN, &Context::default())
        .control
        .unwrap();
    let bound = json_ui::bind(&root, &data, &json_ui::EmptyLibrary);
    let (laid, _) = json_ui::layout_with(
        &bound,
        editor.viewport,
        &json_ui::LayoutEnv {
            text: &Art,
            textures: &Art,
        },
        &editor.view,
    );
    assert!(
        !json_ui::hit_regions(&laid)
            .iter()
            .any(|hit| hit.pressed.as_deref() == Some("hud.card:0"))
    );
    editor.action("hud.visibility:0", &mut []);
    assert!(editor.snapshot(true).hidden_cards.is_empty());
    editor.step_history(false);
    assert_eq!(editor.snapshot(true).hidden_cards, ["disabled"]);
    editor.step_history(true);
    assert!(editor.snapshot(true).hidden_cards.is_empty());
}

#[test]
fn amendment_inspector_and_viewport_bindings_use_gui_pixels() {
    let mut editor = published(serde_json::json!({"inspector_size":[180,120],"cards":[{
        "id":"fixture","width":100,"offset":[40,60],"rows":[]
    }]}));
    editor.selected = Some(0);
    let data = editor.data();
    assert_eq!(
        data.global_value("#hud_inspector_x"),
        Some(&Scalar::Num(156.))
    );
    assert_eq!(
        data.global_value("#hud_inspector_y"),
        Some(&Scalar::Num(60.))
    );
    assert_eq!(
        data.global_value("#hud_viewport_w"),
        Some(&Scalar::Num(900.))
    );
    assert_eq!(
        data.global_value("#hud_viewport_h"),
        Some(&Scalar::Num(600.))
    );
}

#[test]
fn amendment_grid_does_not_snap_movement_and_snap_controls_guides_and_grid() {
    for (grid, snap, bypass) in [
        (true, false, false),
        (false, true, false),
        (true, true, true),
    ] {
        let mut editor = editor(false);
        editor.grid = grid;
        editor.snap = snap;
        editor.bypass_snap = bypass;
        let point = frame(&mut editor, "hud.card:0");
        editor.pointer(point, true, true, &mut []);
        editor.pointer([point[0] + 37., point[1] + 41.], false, true, &mut []);
        let at = editor.card_origin(0);
        let expected = if snap && !bypass {
            [40., 40.]
        } else {
            [37., 41.]
        };
        for axis in 0..2 {
            assert!((at[axis] - expected[axis]).abs() < 0.001);
        }
        editor.pointer([point[0] + 3., point[1] + 3.], false, true, &mut []);
        assert_eq!(
            editor
                .drag
                .as_ref()
                .unwrap()
                .guides
                .iter()
                .any(Option::is_some),
            snap && !bypass
        );
    }
}

#[test]
fn amendment_live_hidden_cards_preserve_draft_visibility_and_seed_new_history() {
    let mut editor = published(serde_json::json!({"cards":[{
        "id":"disabled","visible":false,"rows":[]
    }]}));
    editor.action("hud.visibility:0", &mut []);
    let mut cards = editor.draft.cards.clone();
    cards[0].title = "Fresh".into();
    cards.push(Card {
        id: "new".into(),
        visible: false,
        ..Default::default()
    });
    editor.republish_cards(&cards, true);
    assert_eq!(editor.snapshot(true).hidden_cards, ["new"]);
    editor.step_history(false);
    assert_eq!(editor.snapshot(true).hidden_cards, ["disabled", "new"]);
    editor.action("hud.visibility:1", &mut []);
    editor.step_history(false);
    assert_eq!(editor.snapshot(true).hidden_cards, ["disabled", "new"]);
    assert_eq!(editor.draft.cards[0].title, "Fresh");
}

#[test]
fn amendment_live_inspector_size_updates_bindings_without_rebuilding_catalog() {
    let mut editor = published(serde_json::json!({"inspector_size":[180,120],"cards":[{
        "id":"fixture","width":100,"offset":[700,60],"rows":[]
    }]}));
    editor.selected = Some(0);
    let catalog = Arc::new(template::catalog(&editor.draft, editor.viewport).unwrap());
    editor.catalog = Some(Arc::clone(&catalog));
    let mut preview = editor.draft.clone();
    preview.inspector_size = Some([200., 140.]);
    let mut ui = ModUi {
        mod_hud_editor: Some(editor),
        ..Default::default()
    };
    ui.update_mod_hud_editor(&preview).unwrap();
    let editor = ui.mod_hud_editor.as_mut().unwrap();
    assert!(Arc::ptr_eq(editor.catalog.as_ref().unwrap(), &catalog));
    assert_eq!(
        editor.data().global_value("#hud_inspector_x"),
        Some(&Scalar::Num(484.))
    );
    editor.viewport = [1200., 800.];
    let data = editor.data();
    assert_eq!(
        data.global_value("#hud_inspector_x"),
        Some(&Scalar::Num(816.))
    );
    assert_eq!(
        data.global_value("#hud_viewport_w"),
        Some(&Scalar::Num(1200.))
    );
    assert_eq!(
        data.global_value("#hud_viewport_h"),
        Some(&Scalar::Num(800.))
    );
}
