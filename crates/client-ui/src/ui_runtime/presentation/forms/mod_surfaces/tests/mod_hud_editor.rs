use mod_ui::PanelUiOwner;
use std::sync::Arc;

use mod_ui::test_support::{card_dimensions, card_origin};
use ui::mod_hud::{Anchor, Card, EditorResult, Hud};

use super::super::super::snapshot;
use super::mod_widgets::{content, frame, installed_hud_presentation, presentation};
use crate::ui_runtime::{UiRuntime, presentation::UiPresentationRuntime};

pub(super) fn panel() -> ui::mod_panel::Panel {
    serde_json::from_str(
        r#"{"title":"Personal HUD","toggle_key":"ShiftRight","dark":true,"controls":[]}"#,
    )
    .unwrap()
}
fn editor(p: &mut UiPresentationRuntime, hud: &Hud) {
    p.mod_ui_mut()
        .set_mod_panel(PanelUiOwner::Environment(0), Some(&panel()))
        .unwrap();
    p.mod_ui_mut().set_mod_panel_open(true);
    p.mod_ui_mut().open_mod_hud_editor(hud).unwrap();
}
pub(super) fn point(p: &UiPresentationRuntime, action: &str) -> [f32; 2] {
    let editor = p.mod_ui().mod_hud_editor_state().unwrap();
    let frame = editor.frame.expect("editor rendered");
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
pub(super) fn click(p: &mut UiPresentationRuntime, action: &str) {
    let at = point(p, action);
    p.mod_ui_mut().mod_panel_events(at, true, true);
    p.mod_ui_mut().mod_panel_events(at, false, false);
}
#[test]
fn editor_drag_uses_real_scaled_hit_regions_and_retains_its_catalog() {
    for (size, dpi, gui) in [
        ([1280, 720], 1., 1),
        ([1920, 1080], 1.5, 2),
        ([2560, 1440], 2., 3),
    ] {
        let mut p = presentation(false);
        let runtime = UiRuntime::new(1);
        p.set_gui_scale_preference(Some(gui));
        let mut hud = content();
        hud.cards[0].title.clear();
        editor(&mut p, &hud);
        frame(&mut p, &runtime, size, dpi);
        let at = point(&p, "hud.card:0");
        let catalog = Arc::clone(p.mod_ui().mod_hud_editor_state().unwrap().catalog.unwrap());
        p.mod_ui_mut().mod_panel_events(at, true, true);
        // Pointer capture persists outside the hit region, clamps at both screen edges.
        p.mod_ui_mut()
            .mod_panel_events([10000., 10000.], false, true);
        frame(&mut p, &runtime, size, dpi);
        let draft = p.mod_ui().mod_hud_editor_state().unwrap().draft;
        assert_eq!(draft.cards[0].position, Some([1., 1.]));
        assert!(Arc::ptr_eq(
            &catalog,
            p.mod_ui().mod_hud_editor_state().unwrap().catalog.unwrap()
        ));
        p.mod_ui_mut()
            .mod_panel_events([-1000., -1000.], false, false);
        assert_eq!(
            p.mod_ui().mod_hud_editor_state().unwrap().draft.cards[0].position,
            None
        );
        let draft = &p.mod_ui().mod_hud_editor_state().unwrap().draft.cards[0];
        assert_eq!(draft.anchor, Anchor::TopLeft);
        assert_eq!(draft.offset, [0.; 2]);
        frame(&mut p, &runtime, size, dpi);
        let passes = p.mod_ui().mod_hud_editor_state().unwrap().passes;
        frame(&mut p, &runtime, size, dpi);
        assert_eq!(p.mod_ui().mod_hud_editor_state().unwrap().passes, passes);
        assert!(
            Arc::ptr_eq(
                &catalog,
                p.mod_ui().mod_hud_editor_state().unwrap().catalog.unwrap()
            ),
            "completed anchoring and steady frames retain the catalog"
        );
        p.mod_ui_mut().mod_panel_key("Enter", None);
        let result = p.mod_ui_mut().take_mod_hud_editor_result().unwrap();
        assert!(result.saved && !result.reset);
        assert_eq!(result.placements[0].id, "equipment");
        assert_eq!(result.placements[0].position, Some([0., 0.]));
        assert!(p.mod_ui().mod_panel_open() && !p.mod_ui().mod_hud_editor_open());
    }
}
#[test]
fn custom_row_layout_drag_bounds_match_the_rendered_card_surface() {
    for (layout, width, row_height, icon_size) in [
        (ui::mod_hud::RowLayout::StackedText, 128., 34., 22.),
        (ui::mod_hud::RowLayout::IconRight, 60., 28., 20.),
    ] {
        for scale in [0.5, 1., 2.] {
            let mut p = presentation(false);
            let mut hud = content();
            let card = &mut hud.cards[0];
            card.title.clear();
            card.scale = scale;
            card.row_layout = layout;
            card.width = width;
            card.row_height = row_height;
            card.icon_size = icon_size;
            editor(&mut p, &hud);
            frame(&mut p, &UiRuntime::new(1), [1280, 720], 1.);
            let editor = p.mod_ui().mod_hud_editor_state().unwrap();
            let rendered = editor.frame.unwrap();
            let hit = rendered
                .hits
                .iter()
                .find(|hit| hit.pressed.as_deref() == Some("hud.card:0"))
                .expect("native card drag region");
            let surface = p
                .last_frame
                .as_ref()
                .unwrap()
                .nodes
                .iter()
                .find_map(|node| {
                    let ui::UiVisual::Mesh(mesh) = node.visual() else {
                        return None;
                    };
                    mesh.vertices()
                        .iter()
                        .any(|vertex| vertex.color == [11, 13, 17, 209])
                        .then(|| node.bounds())
                })
                .expect("rendered card surface");
            assert!(
                (surface.max().x() - surface.min().x() - hit.rect.w as f32 * rendered.scale).abs()
                    < 0.01
            );
            assert!(
                (surface.max().y() - surface.min().y() - hit.rect.h as f32 * rendered.scale).abs()
                    < 0.01
            );
            let at = point(&p, "hud.card:0");
            p.mod_ui_mut().mod_panel_events(at, true, true);
            p.mod_ui_mut().mod_panel_events([10000.; 2], false, false);
            assert_eq!(
                p.mod_ui().mod_hud_editor_state().unwrap().draft.cards[0].position,
                None
            );
            assert_eq!(
                p.mod_ui().mod_hud_editor_state().unwrap().draft.cards[0].anchor,
                Anchor::BottomRight
            );
        }
    }
}
#[test]
fn editor_selection_preserves_legacy_placement_and_reset_is_a_draft() {
    let mut p = presentation(false);
    let mut hud = content();
    hud.cards[0].title.clear();
    hud.cards[0].anchor = Anchor::BottomRight;
    hud.cards[0].offset = [6., 40.];
    hud.cards[0].reset_anchor = Some(Anchor::TopLeft);
    hud.cards[0].reset_offset = Some([6., 6.]);
    editor(&mut p, &hud);
    frame(&mut p, &UiRuntime::new(1), [1280, 720], 1.);
    click(&mut p, "hud.card:0");
    assert_eq!(
        p.mod_ui().mod_hud_editor_state().unwrap().draft.cards[0].position,
        None
    );
    click(&mut p, "hud.reset");
    let draft = &p.mod_ui().mod_hud_editor_state().unwrap().draft.cards[0];
    assert_eq!(draft.anchor, Anchor::TopLeft);
    assert_eq!(draft.offset, [6., 6.]);
    p.mod_ui_mut().mod_panel_key("Escape", None);
    assert_eq!(
        p.mod_ui_mut().take_mod_hud_editor_result(),
        Some(EditorResult::default())
    );
    editor(&mut p, &hud);
    frame(&mut p, &UiRuntime::new(1), [1280, 720], 1.);
    click(&mut p, "hud.reset");
    p.mod_ui_mut().mod_panel_key("Enter", None);
    let result = p.mod_ui_mut().take_mod_hud_editor_result().unwrap();
    assert!(result.saved && result.reset);
    assert_eq!(result.placements[0].anchor, Anchor::TopLeft);
    assert_eq!(result.placements[0].offset, [6., 6.]);
    assert!(result.placements[0].position.is_some());
    assert!(p.mod_ui().mod_panel_open());
}
#[test]
fn editor_grid_is_optional_and_nudging_stays_inside_available_travel() {
    let mut p = presentation(false);
    let mut hud = content();
    hud.cards[0].title.clear();
    editor(&mut p, &hud);
    frame(&mut p, &UiRuntime::new(1), [1920, 1080], 1.);
    click(&mut p, "hud.grid");
    assert!(p.mod_ui().mod_hud_editor_state().unwrap().grid);
    assert!(!p.mod_ui().mod_hud_editor_state().unwrap().snap);
    click(&mut p, "hud.snap");
    assert!(p.mod_ui().mod_hud_editor_state().unwrap().snap);
    let at = point(&p, "hud.card:0");
    p.mod_ui_mut().mod_panel_events(at, true, true);
    p.mod_ui_mut()
        .mod_panel_events([at[0] + 33., at[1] + 41.], false, false);
    let e = p.mod_ui().mod_hud_editor_state().unwrap();
    let origin = card_origin(&e.draft.cards[0], e.viewport);
    assert!((origin[0] / 8. - (origin[0] / 8.).round()).abs() < 0.001);
    assert!((origin[1] / 8. - (origin[1] / 8.).round()).abs() < 0.001);
    p.mod_ui_mut().mod_panel_key("ArrowRight", None);
    let e = p.mod_ui().mod_hud_editor_state().unwrap();
    let moved = card_origin(&e.draft.cards[0], e.viewport);
    assert!((moved[0] - origin[0] - 8.).abs() < 0.001);
}

/// Real pointer releases in all nine regions retain their GUI placement after a window resize.
#[test]
fn anchored_editor_drops_keep_offsets_across_window_and_gui_scale_changes() {
    for (row, anchors) in [
        [Anchor::TopLeft, Anchor::TopCenter, Anchor::TopRight],
        [Anchor::Left, Anchor::Center, Anchor::Right],
        [
            Anchor::BottomLeft,
            Anchor::BottomCenter,
            Anchor::BottomRight,
        ],
    ]
    .into_iter()
    .enumerate()
    {
        for (column, expected) in anchors.into_iter().enumerate() {
            let mut p = presentation(false);
            p.set_gui_scale_preference(Some(1));
            let mut hud = content();
            hud.autosave = true;
            hud.cards[0].width = 100.;
            hud.cards[0].title.clear();
            editor(&mut p, &hud);
            frame(&mut p, &UiRuntime::new(1), [1920, 1080], 1.);
            let e = p.mod_ui().mod_hud_editor_state().unwrap();
            let size = card_dimensions(&e.draft.cards[0]);
            let before = card_origin(&e.draft.cards[0], e.viewport);
            let target: [f64; 2] = std::array::from_fn(|axis| {
                e.viewport[axis] * ([column, row][axis] as f64 + 0.5) / 3. - size[axis] * 0.5
            });
            let px = e.frame.unwrap().scale;
            let at = point(&p, "hud.card:0");
            let destination =
                std::array::from_fn(|axis| at[axis] + ((target[axis] - before[axis]) as f32 * px));
            p.mod_ui_mut().mod_panel_events(at, true, true);
            p.mod_ui_mut().mod_panel_events(destination, false, false);
            let saved = p.mod_ui_mut().take_mod_hud_editor_result().unwrap();
            let placement = &saved.placements[0];
            assert_eq!(placement.anchor, expected);
            assert!(
                p.mod_ui().mod_hud_editor_state().unwrap().draft.cards[0]
                    .position
                    .is_none()
            );
            let e = p.mod_ui().mod_hud_editor_state().unwrap();
            let dropped = card_origin(&e.draft.cards[0], e.viewport);
            for axis in 0..2 {
                assert!((dropped[axis] - target[axis]).abs() < 0.001);
            }
            for (window, gui) in [([2560, 1440], 1), ([2560, 1440], 2)] {
                p.set_gui_scale_preference(Some(gui));
                frame(&mut p, &UiRuntime::new(1), window, 1.);
                let e = p.mod_ui().mod_hud_editor_state().unwrap();
                let card = &e.draft.cards[0];
                assert_eq!(card.anchor, expected);
                assert_eq!(card.offset, placement.offset);
                let origin = card_origin(card, e.viewport);
                for axis in 0..2 {
                    let fraction = [column, row][axis] as f64 * 0.5;
                    let distance =
                        origin[axis] + size[axis] * fraction - e.viewport[axis] * fraction;
                    let direction = if fraction == 1. { -1. } else { 1. };
                    assert!(
                        (distance * direction - f64::from(placement.offset[axis])).abs() < 0.001
                    );
                }
            }
        }
    }
}
#[test]
fn large_editor_viewport_draws_a_bounded_subset_of_the_same_eight_pixel_grid() {
    let mut p = presentation(false);
    p.set_gui_scale_preference(Some(1));
    let mut hud = content();
    hud.cards[0].title.clear();
    editor(&mut p, &hud);
    frame(&mut p, &UiRuntime::new(1), [8192, 2160], 1.);
    click(&mut p, "hud.grid");
    frame(&mut p, &UiRuntime::new(1), [8192, 2160], 1.);
    let e = p.mod_ui().mod_hud_editor_state().unwrap();
    assert!(
        e.viewport[0] > 1536.,
        "exercise a viewport that needs bounded grid decimation"
    );
    let px = e.frame.unwrap().scale;
    let lines: Vec<_> = p
        .last_frame
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .filter_map(|node| {
            let ui::UiVisual::Mesh(mesh) = node.visual() else {
                return None;
            };
            mesh.vertices()
                .iter()
                .any(|v| v.color == [255, 255, 255, 23])
                .then(|| node.bounds())
        })
        .collect();
    assert!(
        !lines.is_empty() && lines.len() <= 384,
        "bounded visible grid lines"
    );
    for line in lines {
        let axis = usize::from(line.max().x() - line.min().x() > px);
        let at = if axis == 0 {
            line.min().x()
        } else {
            line.min().y()
        };
        let grid = at / px / 8.;
        assert!(
            (grid - grid.round()).abs() < 0.001,
            "visible lines agree with eight pixel snapping"
        );
    }
}
#[test]
fn editor_closure_and_invalid_preview_release_drag_without_persisting() {
    let mut p = presentation(false);
    let hud = content();
    editor(&mut p, &hud);
    frame(&mut p, &UiRuntime::new(1), [1280, 720], 1.);
    let at = point(&p, "hud.card:0");
    p.mod_ui_mut().mod_panel_events(at, true, true);
    let mut invalid = hud.clone();
    invalid.cards[0].position = Some([f32::NAN, 0.]);
    assert!(p.mod_ui_mut().open_mod_hud_editor(&invalid).is_err());
    assert!(p.mod_ui().mod_hud_editor_open());
    p.mod_ui_mut().set_mod_panel_open(false);
    assert!(!p.mod_ui().mod_hud_editor_open());
    assert_eq!(
        p.mod_ui_mut().take_mod_hud_editor_result(),
        Some(EditorResult::default())
    );
    p.mod_ui_mut().mod_panel_events([1000., 1000.], false, true);
    assert!(!p.mod_ui().mod_panel_open());
}
#[test]
fn editor_style_recolors_outlines_and_resize_handles() {
    let emits = |p: &UiPresentationRuntime, color: [u8; 4]| {
        p.last_frame.as_ref().unwrap().nodes.iter().any(|node| {
            matches!(node.visual(), ui::UiVisual::Mesh(mesh)
                if mesh.vertices().iter().any(|vertex| vertex.color == color))
        })
    };
    let mut hud = content();
    hud.cards[0].title.clear();
    hud.resizable = true;
    hud.editor_style = Some(ui::mod_hud::EditorStyle {
        outline: [0., 0., 1., 1.],
        selected: [1., 0., 1., 1.],
        handle: [1., 1., 0., 1.],
        handle_radius: 2.5,
        ..Default::default()
    });
    let mut p = presentation(false);
    editor(&mut p, &hud);
    let runtime = UiRuntime::new(1);
    frame(&mut p, &runtime, [1280, 720], 1.);
    assert!(!emits(&p, [0, 0, 255, 255]), "idle cards draw no outline");
    assert!(emits(&p, [255, 255, 0, 255]), "resize handle");
    let at = point(&p, "hud.card:0");
    p.mod_ui_mut().mod_panel_events(at, false, false);
    frame(&mut p, &runtime, [1280, 720], 1.);
    assert!(emits(&p, [0, 0, 255, 255]), "hovered outline");
    assert!(
        !emits(&p, [76, 217, 255, 255]),
        "built-in handle color is replaced"
    );
    click(&mut p, "hud.card:0");
    frame(&mut p, &runtime, [1280, 720], 1.);
    assert!(emits(&p, [255, 0, 255, 255]), "selected outline");
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
    let catalog = Arc::clone(p.mod_ui().mod_widgets_state().unwrap().catalog);
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
        p.mod_ui().mod_widgets_state().unwrap().catalog
    ));
    for (size, dpi, scale) in [
        ([1280, 720], 1., 0.5),
        ([1920, 1080], 1.5, 1.),
        ([2560, 1440], 2., 2.),
    ] {
        hud.cards[0].scale = scale;
        p.set_mod_hud(Some(&hud)).unwrap();
        frame(&mut p, &runtime, size, dpi);
        let widgets = p.mod_ui().mod_widgets_state().unwrap();
        let size = card_dimensions(&hud.cards[0]);
        let at = card_origin(&hud.cards[0], widgets.viewport);
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
        offset: [8., 30.],
        rows: vec![ui::mod_hud::Row {
            localized: false,
            formatted: false,
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
    p.mod_ui_mut().mod_panel_events(at, true, true);
    p.mod_ui_mut()
        .mod_panel_events([at[0] + 450., at[1] + 220.], false, false);
    snapshot::write(
        &frame(&mut p, &runtime, [1920, 1080], 1.),
        "hud-layout-dragged",
    );
    p.mod_ui_mut().mod_panel_key("Enter", None);
    let result = p.mod_ui_mut().take_mod_hud_editor_result().unwrap();
    for placement in result.placements {
        let card = hud.cards.iter_mut().find(|c| c.id == placement.id).unwrap();
        card.anchor = placement.anchor;
        card.offset = placement.offset;
        card.scale = placement.scale;
        card.position = None;
    }
    p.mod_ui_mut().set_mod_panel_open(false);
    p.set_mod_hud(Some(&hud)).unwrap();
    snapshot::write(
        &frame(&mut p, &runtime, [1920, 1080], 1.),
        "hud-layout-saved",
    );
    snapshot::write(
        &frame(&mut p, &runtime, [2560, 1440], 1.),
        "hud-layout-resized",
    );
}
