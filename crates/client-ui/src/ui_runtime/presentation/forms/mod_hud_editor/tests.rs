use super::super::{
    mod_widgets::tests::{content, frame, installed_hud_presentation, presentation},
    snapshot,
};
use super::*;
use ui::mod_hud::{Anchor, Card};

fn panel() -> ui::mod_panel::Panel {
    serde_json::from_str(
        r#"{"title":"Personal HUD","toggle_key":"ShiftRight","dark":true,"controls":[]}"#,
    )
    .unwrap()
}
fn editor(p: &mut UiPresentationRuntime, hud: &Hud) {
    p.set_mod_panel(Some(&panel())).unwrap();
    p.set_mod_panel_open(true);
    p.open_mod_hud_editor(hud).unwrap();
}
fn point(p: &UiPresentationRuntime, action: &str) -> [f32; 2] {
    let editor = p.form_presentation.mod_hud_editor.as_ref().unwrap();
    let frame = editor.frame.as_ref().expect("editor rendered");
    let hit = frame
        .hits
        .iter()
        .find(|hit| hit.pressed.as_deref() == Some(action))
        .expect("native hit region");
    [
        (hit.rect.x + hit.rect.w * 0.5) as f32 * frame.scale + frame.origin[0],
        (hit.rect.y + hit.rect.h * 0.5) as f32 * frame.scale + frame.origin[1],
    ]
}
fn click(p: &mut UiPresentationRuntime, action: &str) {
    let at = point(p, action);
    p.mod_panel_events(at, true, true);
    p.mod_panel_events(at, false, false);
}
#[test]
fn editor_drag_uses_real_scaled_hit_regions_and_retains_its_catalog() {
    for (size, dpi, gui) in [
        ([1280, 720], 1., 1),
        ([1920, 1080], 1.5, 2),
        ([2560, 1440], 2., 3),
    ] {
        let mut p = presentation(false);
        p.set_gui_scale_preference(Some(gui));
        let mut hud = content();
        hud.cards[0].title.clear();
        editor(&mut p, &hud);
        frame(&mut p, &UiRuntime::new(1), size, dpi);
        let at = point(&p, "hud.card:0");
        let catalog = Arc::clone(
            p.form_presentation
                .mod_hud_editor
                .as_ref()
                .unwrap()
                .catalog
                .as_ref()
                .unwrap(),
        );
        p.mod_panel_events(at, true, true);
        // Pointer capture persists outside the hit region, clamps at both screen edges.
        p.mod_panel_events([10000., 10000.], false, true);
        frame(&mut p, &UiRuntime::new(1), size, dpi);
        let draft = &p.form_presentation.mod_hud_editor.as_ref().unwrap().draft;
        assert_eq!(draft.cards[0].position, Some([1., 1.]));
        assert!(Arc::ptr_eq(
            &catalog,
            p.form_presentation
                .mod_hud_editor
                .as_ref()
                .unwrap()
                .catalog
                .as_ref()
                .unwrap()
        ));
        p.mod_panel_events([-1000., -1000.], false, false);
        assert_eq!(
            p.form_presentation
                .mod_hud_editor
                .as_ref()
                .unwrap()
                .draft
                .cards[0]
                .position,
            Some([0., 0.])
        );
        p.mod_panel_key("Enter", None);
        let result = p.take_mod_hud_editor_result().unwrap();
        assert!(result.saved && !result.reset);
        assert_eq!(result.placements[0].id, "equipment");
        assert_eq!(result.placements[0].position, Some([0., 0.]));
        assert!(p.mod_panel_open() && !p.mod_hud_editor_open());
    }
}
#[test]
fn editor_selection_preserves_legacy_placement_and_reset_is_a_draft() {
    let mut p = presentation(false);
    let mut hud = content();
    hud.cards[0].title.clear();
    hud.cards[0].anchor = Anchor::BottomRight;
    hud.cards[0].offset = [-6., -40.];
    hud.cards[0].reset_anchor = Some(Anchor::TopLeft);
    hud.cards[0].reset_offset = Some([6., 6.]);
    editor(&mut p, &hud);
    frame(&mut p, &UiRuntime::new(1), [1280, 720], 1.);
    click(&mut p, "hud.card:0");
    assert_eq!(
        p.form_presentation
            .mod_hud_editor
            .as_ref()
            .unwrap()
            .draft
            .cards[0]
            .position,
        None
    );
    click(&mut p, "hud.reset");
    let draft = &p
        .form_presentation
        .mod_hud_editor
        .as_ref()
        .unwrap()
        .draft
        .cards[0];
    assert_eq!(draft.anchor, Anchor::TopLeft);
    assert_eq!(draft.offset, [6., 6.]);
    p.mod_panel_key("Escape", None);
    assert_eq!(
        p.take_mod_hud_editor_result(),
        Some(EditorResult::default())
    );
    editor(&mut p, &hud);
    frame(&mut p, &UiRuntime::new(1), [1280, 720], 1.);
    click(&mut p, "hud.reset");
    p.mod_panel_key("Enter", None);
    let result = p.take_mod_hud_editor_result().unwrap();
    assert!(result.saved && result.reset);
    assert_eq!(result.placements[0].position, None);
    assert!(p.mod_panel_open());
}
#[test]
fn editor_grid_is_optional_and_nudging_stays_inside_available_travel() {
    let mut p = presentation(false);
    let mut hud = content();
    hud.cards[0].title.clear();
    editor(&mut p, &hud);
    frame(&mut p, &UiRuntime::new(1), [1920, 1080], 1.);
    click(&mut p, "hud.grid");
    assert!(p.form_presentation.mod_hud_editor.as_ref().unwrap().snap);
    let at = point(&p, "hud.card:0");
    p.mod_panel_events(at, true, true);
    p.mod_panel_events([at[0] + 33., at[1] + 41.], false, false);
    let e = p.form_presentation.mod_hud_editor.as_ref().unwrap();
    let origin = cards::origin(&e.draft.cards[0], e.viewport);
    assert!((origin[0] / 8. - (origin[0] / 8.).round()).abs() < 0.001);
    assert!((origin[1] / 8. - (origin[1] / 8.).round()).abs() < 0.001);
    p.mod_panel_key("ArrowRight", None);
    let e = p.form_presentation.mod_hud_editor.as_ref().unwrap();
    let moved = cards::origin(&e.draft.cards[0], e.viewport);
    assert!((moved[0] - origin[0] - 8.).abs() < 0.001);
}
#[test]
fn editor_closure_and_invalid_preview_release_drag_without_persisting() {
    let mut p = presentation(false);
    let hud = content();
    editor(&mut p, &hud);
    frame(&mut p, &UiRuntime::new(1), [1280, 720], 1.);
    let at = point(&p, "hud.card:0");
    p.mod_panel_events(at, true, true);
    let mut invalid = hud.clone();
    invalid.cards[0].position = Some([f32::NAN, 0.]);
    assert!(p.open_mod_hud_editor(&invalid).is_err());
    assert!(p.mod_hud_editor_open());
    p.set_mod_panel_open(false);
    assert!(!p.mod_hud_editor_open());
    assert_eq!(
        p.take_mod_hud_editor_result(),
        Some(EditorResult::default())
    );
    p.mod_panel_events([1000., 1000.], false, true);
    assert!(!p.mod_panel_open());
}
#[test]
fn normalized_titleless_cards_keep_foreground_opaque_and_fit_after_resize() {
    let mut p = presentation(false);
    let mut hud = content();
    hud.cards[0].title.clear();
    hud.cards[0].position = Some([0.5, 1.]);
    hud.cards[0].background_opacity = 0.;
    hud.cards[0].rows[0].color = [1., 0., 0., 1.];
    let runtime = UiRuntime::new(1);
    p.set_mod_hud(Some(&hud)).unwrap();
    let clear = frame(&mut p, &runtime, [1280, 720], 1.);
    let catalog = Arc::clone(&p.form_presentation.mod_widgets.as_ref().unwrap().catalog);
    for opacity in [0., 0.35, 1.] {
        hud.cards[0].background_opacity = opacity;
        p.set_mod_hud(Some(&hud)).unwrap();
        frame(&mut p, &runtime, [1280, 720], 1.);
        let alpha = (opacity * 255.).round() as u8;
        assert!(
            p.last_frame.as_ref().unwrap().nodes.iter().any(|node| {
                matches!(node.visual(), ui::UiVisual::Mesh(mesh) if mesh.vertices().iter().any(|v|
                v.color == [11,13,17,alpha]))
            }),
            "card surface emits its own opacity {opacity}"
        );
        assert!(p.last_frame.as_ref().unwrap().nodes.iter().any(|node| {
            matches!(node.visual(), ui::UiVisual::Mesh(mesh) if mesh.vertices().iter().any(|v|v.color==[255,0,0,255]))
        }), "background opacity {opacity} leaves progress fill opaque");
    }
    hud.cards[0].background_opacity = 1.;
    p.set_mod_hud(Some(&hud)).unwrap();
    let opaque = frame(&mut p, &runtime, [1280, 720], 1.);
    assert!(
        snapshot::rasterize(&clear) != snapshot::rasterize(&opaque),
        "background opacity changes visible pixels"
    );
    assert!(Arc::ptr_eq(
        &catalog,
        &p.form_presentation.mod_widgets.as_ref().unwrap().catalog
    ));
    for (size, dpi, scale) in [
        ([1280, 720], 1., 0.5),
        ([1920, 1080], 1.5, 1.),
        ([2560, 1440], 2., 2.),
    ] {
        hud.cards[0].scale = scale;
        p.set_mod_hud(Some(&hud)).unwrap();
        frame(&mut p, &runtime, size, dpi);
        let widgets = p.form_presentation.mod_widgets.as_ref().unwrap();
        let size = cards::dimensions(&hud.cards[0]);
        let at = cards::origin(&hud.cards[0], widgets.viewport);
        assert!(
            (size[1] - 24. * f64::from(scale)).abs() < 0.001,
            "titleless header collapses"
        );
        assert!(
            at[0] >= 0.
                && at[1] >= 0.
                && at[0] + size[0] <= widgets.viewport[0] + 0.001
                && at[1] + size[1] <= widgets.viewport[1] + 0.001
        );
        let nodes = &p.last_frame.as_ref().unwrap().nodes;
        assert!(nodes.iter().any(|n|matches!(n.visual(),ui::UiVisual::Mesh(m) if m.vertices().iter().any(|v|v.color==[255,0,0,255]))),"durability foreground remains opaque");
    }
}
#[test]
fn hud_grid_editor_snapshot_with_real_carrier() {
    let Some(mut p) = installed_hud_presentation() else {
        eprintln!(
            "skipping hud_grid_editor_snapshot_with_real_carrier: installed local carriers unavailable (make assets)"
        );
        return;
    };
    p.set_gui_scale_preference(Some(2));
    let runtime = UiRuntime::new(1);
    let mut hud = content();
    let mut duplicate = hud.cards[0].rows[0].clone();
    duplicate.label = "Chestplate".into();
    duplicate.item = Some("minecraft:diamond_chestplate".into());
    duplicate.value = "41%".into();
    duplicate.progress = Some(0.41);
    hud.cards[0].rows.push(duplicate);
    let second = Card {
        id: "effects".into(),
        title: "Effects".into(),
        anchor: Anchor::TopRight,
        offset: [-8., 30.],
        rows: vec![ui::mod_hud::Row {
            label: "Speed II".into(),
            value: "1:42".into(),
            item: None,
            effect_id: Some(1),
            metadata: 0,
            progress: None,
            color: [1.; 4],
        }],
        ..Default::default()
    };
    hud.cards.push(second.clone());
    p.set_mod_hud(Some(&hud)).unwrap();
    snapshot::write(
        &frame(&mut p, &runtime, [1920, 1080], 1.),
        "hud-layout-before",
    );
    for card in &mut hud.cards {
        card.editor_label = card.title.clone();
        card.title.clear();
        card.background_opacity = 0.35;
    }
    p.set_mod_hud(Some(&hud)).unwrap();
    snapshot::write(
        &frame(&mut p, &runtime, [1920, 1080], 1.),
        "hud-layout-titleless",
    );
    editor(&mut p, &hud);
    frame(&mut p, &runtime, [1920, 1080], 1.);
    click(&mut p, "hud.grid");
    snapshot::write(
        &frame(&mut p, &runtime, [1920, 1080], 1.),
        "hud-layout-editor",
    );
    let at = point(&p, "hud.card:0");
    p.mod_panel_events(at, true, true);
    p.mod_panel_events([at[0] + 450., at[1] + 220.], false, false);
    snapshot::write(
        &frame(&mut p, &runtime, [1920, 1080], 1.),
        "hud-layout-dragged",
    );
    p.mod_panel_key("Enter", None);
    let result = p.take_mod_hud_editor_result().unwrap();
    for placement in result.placements {
        hud.cards
            .iter_mut()
            .find(|c| c.id == placement.id)
            .unwrap()
            .position = placement.position;
    }
    p.set_mod_panel_open(false);
    p.set_mod_hud(Some(&hud)).unwrap();
    snapshot::write(
        &frame(&mut p, &runtime, [1920, 1080], 1.),
        "hud-layout-saved",
    );
}
