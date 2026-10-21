use super::*;
use collection_tests::{Metrics, four_column_catalogue};
use json_ui::{FormRender, LayoutEnv};
use serde_json::json;

const OWNER: PanelUiOwner = PanelUiOwner::Environment(0);

/// Publishes a patch through the same retained replacement path as the guest host.
fn patch(ui: &mut ModUi, value: serde_json::Value) {
    let panel = PanelPatch::parse(&value.to_string())
        .unwrap()
        .apply(&ui.mod_panel.as_ref().unwrap().panel, &Default::default())
        .unwrap();
    ui.set_mod_panel(OWNER, Some(&panel)).unwrap();
}

use ui::mod_panel::PanelPatch;

/// Requests an immutable item, independently of its current displayed position.
fn reveal(ui: &mut ModUi, item: u32) {
    patch(
        ui,
        json!({"collection_reveal":{"collection":"modules","item":item}}),
    );
}

/// Runs the production reveal, binding and virtualization sequence without font carriers.
fn render(ui: &mut ModUi) -> &FormRender {
    let panel = ui.mod_panel.as_mut().unwrap();
    if panel.catalog.is_none() {
        panel.catalog = Some(Arc::new(
            template::catalog(&panel.panel, [452., 289.], 0, 0, 1, None)
                .unwrap()
                .0,
        ));
    }
    let catalog = panel.catalog.as_ref().unwrap();
    let env = LayoutEnv {
        text: &Metrics,
        textures: &Metrics,
    };
    panel.collection_reveals.apply(
        &panel.panel,
        catalog,
        &mut panel.view,
        &mut panel.motion,
        &env,
    );
    panel
        .screen
        .virtualize_clipped_grid("modules", ui::mod_panel::SurfaceCollection::INITIAL_WINDOW);
    let rendered = panel
        .screen
        .render_shared_with(
            SCREEN,
            catalog,
            crate::personal_context(),
            Arc::clone(&panel.data),
            ([452., 289.], 1., [0; 3]),
            &env,
            &panel.view,
        )
        .unwrap();
    let mut hits = rendered.hits.to_vec();
    surface::resolve_collection_hits(panel.panel.surface.as_ref().unwrap(), &mut hits);
    panel.frame = Some(EngineFrame {
        hits: hits.into(),
        report: rendered.report.clone(),
        cancel_target: None,
        origin: [0.; 2],
        scale: 1.,
        panel: None,
        edit_texts: Vec::new(),
        top: Vec::new(),
    });
    rendered
}

/// Reads the native collection's actual offset, after layout has clamped it.
fn offset(ui: &ModUi) -> f64 {
    ui.mod_panel
        .as_ref()
        .unwrap()
        .frame
        .as_ref()
        .unwrap()
        .report
        .scrolls
        .values()
        .next()
        .unwrap()
        .offset
}

/// Checks the whole actionable cell, including its card padding, against the viewport.
fn visible(ui: &ModUi, item: u32) {
    let frame = ui.mod_panel.as_ref().unwrap().frame.as_ref().unwrap();
    let viewport = frame
        .report
        .scrolls
        .values()
        .next()
        .unwrap()
        .viewport_rect
        .unwrap();
    let action = format!("mod.control:{item}");
    let hit = frame
        .hits
        .iter()
        .find(|hit| hit.pressed.as_deref() == Some(action.as_str()))
        .unwrap();
    assert!(
        hit.rect.y >= viewport[1],
        "cell top is clipped: {:?}",
        hit.rect
    );
    assert!(
        hit.rect.y + hit.rect.h <= viewport[1] + viewport[3],
        "cell bottom is clipped: {:?}",
        hit.rect
    );
}

/// Builds an open retained four-column grid with no carrier dependencies.
fn grid(motion: bool) -> ModUi {
    let mut ui = ModUi::default();
    let mut panel = four_column_catalogue();
    panel.motion = motion;
    ui.set_mod_panel(OWNER, Some(&panel)).unwrap();
    ui.set_mod_panel_open(true);
    ui
}

/// A first-frame reveal reaches virtualized rows, moves minimally, and can return upward.
#[test]
fn collection_reveal_moves_below_and_above_and_preserves_visible_items() {
    for reduced in [false, true] {
        let mut ui = grid(true);
        ui.configure_motion(!reduced);
        reveal(&mut ui, 12);
        render(&mut ui);
        assert_eq!(offset(&ui), 99.);
        visible(&ui, 12);
        reveal(&mut ui, 8);
        render(&mut ui);
        assert_eq!(offset(&ui), 99.);
        visible(&ui, 8);
        reveal(&mut ui, 65);
        render(&mut ui);
        assert_eq!(offset(&ui), 1360.);
        visible(&ui, 65);
        reveal(&mut ui, 0);
        render(&mut ui);
        assert_eq!(offset(&ui), 0.);
        visible(&ui, 0);
    }
}

/// A consumed request never overrides the pointer; an identical new request reveals again.
#[test]
fn collection_reveal_wheel_then_repeat_cancels_easing_and_reuses_steady_frames() {
    let mut ui = grid(true);
    reveal(&mut ui, 12);
    let nodes = Arc::clone(&render(&mut ui).nodes);
    let passes = ui.mod_panel.as_ref().unwrap().screen.passes;
    let catalog = Arc::clone(ui.mod_panel.as_ref().unwrap().catalog.as_ref().unwrap());
    for _ in 0..4 {
        assert!(Arc::ptr_eq(&nodes, &render(&mut ui).nodes));
        assert_eq!(ui.mod_panel.as_ref().unwrap().screen.passes, passes);
    }
    let published = ui.mod_panel.as_ref().unwrap().panel.clone();
    assert!(ui.scroll_mod_panel([10., 10.], -200., true));
    ui.set_mod_panel(OWNER, Some(&published)).unwrap();
    patch(&mut ui, json!({"capture_key":true}));
    render(&mut ui);
    assert_eq!(offset(&ui), 299.);
    assert!(ui.scroll_mod_panel([10., 10.], -1., false));
    reveal(&mut ui, 12);
    render(&mut ui);
    assert_eq!(offset(&ui), 291.);
    visible(&ui, 12);
    assert!(!ui.mod_panel.as_ref().unwrap().motion.active());
    assert!(Arc::ptr_eq(
        &catalog,
        ui.mod_panel.as_ref().unwrap().catalog.as_ref().unwrap()
    ));
    let panel = ui.mod_panel.as_mut().unwrap();
    let (_, allocations) = test_alloc::measure(|| {
        panel.collection_reveals.apply(
            &panel.panel,
            &catalog,
            &mut panel.view,
            &mut panel.motion,
            &LayoutEnv {
                text: &Metrics,
                textures: &Metrics,
            },
        );
    });
    assert_eq!(allocations.calls, 0);
}

/// Filtered order determines the row, while absent items are ignored once per request.
#[test]
fn collection_reveal_uses_filtered_order_and_counts_ignored_requests_once() {
    let mut ui = grid(false);
    patch(
        &mut ui,
        json!({"collection_order":{"modules":(33..66).rev().collect::<Vec<_>>()},
        "collection_reveal":{"collection":"modules","item":33}}),
    );
    render(&mut ui);
    assert_eq!(offset(&ui), 584.);
    visible(&ui, 33);
    reveal(&mut ui, 12);
    for _ in 0..4 {
        render(&mut ui);
    }
    assert_eq!(offset(&ui), 584.);
    assert_eq!(ui.mod_panel.as_ref().unwrap().collection_reveals.ignored, 1);
    reveal(&mut ui, 12);
    render(&mut ui);
    assert_eq!(ui.mod_panel.as_ref().unwrap().collection_reveals.ignored, 2);
    patch(
        &mut ui,
        json!({"collection_order":{"modules":[]},
        "collection_reveal":{"collection":"modules","item":33}}),
    );
    render(&mut ui);
    assert_eq!(offset(&ui), 0.);
    assert_eq!(ui.mod_panel.as_ref().unwrap().collection_reveals.ignored, 3);
}

/// A complete replacement cancels requests belonging to the content it superseded.
#[test]
fn collection_reveal_replacement_cancels_unpainted_requests() {
    let mut ui = grid(false);
    reveal(&mut ui, 12);
    ui.set_mod_panel(OWNER, Some(&four_column_catalogue()))
        .unwrap();
    render(&mut ui);
    assert_eq!(offset(&ui), 0.);
    reveal(&mut ui, 12);
    render(&mut ui);
    assert_eq!(offset(&ui), 99.);
    visible(&ui, 12);
}
