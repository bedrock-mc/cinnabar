//! Chat link hits land on the glyphs the engine painted, and Java HUD rows keep their height.

use std::sync::Arc;

use json_ui::{Draw, ViewState};
use ui_engine::engine::{FormEngine, ScreenArt, screen::PaintFrame};
use ui_engine::text_metrics::TextMetrics;
use ui_screens::test_support::history_node;

use crate::ui_runtime::presentation::UiPresentationRuntime;

mod hud_rows;

#[test]
fn link_hits_follow_the_actual_default_and_named_label_paint_origin() {
    for named in [false, true] {
        let base = ui_engine::test_support::fixture_font();
        let font = if named {
            Arc::new(base.with_named_font("alternative", &base).unwrap())
        } else {
            base
        };
        for gui_scale in [1u8, 2] {
            let physical = [800, 600].map(|side| side * u32::from(gui_scale));
            let metrics = TextMetrics::for_viewport(
                physical,
                ui::DpiScale::new(1.0).unwrap(),
                Some(gui_scale),
            );
            let px = metrics.scale.get() * ui::FONT_DESIGN_PIXEL_TEXELS as f32;
            let mut presentation = UiPresentationRuntime::new(Arc::clone(&font)).unwrap();
            let engine = FormEngine::new(
                crate::test_support::mini_carrier(),
                Default::default(),
                0,
                super::screen_registry::registry(),
                crate::ui_runtime::presentation::forms::texture_sources::downloads(),
                crate::ui_runtime::presentation::forms::texture_sources::image_work(),
            );
            let mut node = history_node("https://example.com", 600.0);
            if let Draw::Text { options, .. } = &mut node.draw {
                options.font_type = named.then(|| "alternative".into());
            }
            let render = json_ui::FormRender {
                bound: json_ui::ResolvedControl {
                    name: "fixture".into(),
                    control_type: Some("panel".into()),
                    base: None,
                    unresolved_base: None,
                    properties: Default::default(),
                    children: Vec::new(),
                    factory: None,
                },
                nodes: vec![node.clone()],
                hits: Arc::from([]),
                report: Default::default(),
                cancel_target: None,
                root_panel: None,
            };
            let mut painted = Vec::new();
            let mut next = 1;
            engine
                .draw(
                    ScreenArt::default(),
                    &mut PaintFrame {
                        layouts: &mut presentation.layouts,
                        font: &font,
                        metrics,
                        solid_page: presentation.solid_texture_page,
                        safe_area: ui::SafeArea::ZERO,
                        content: physical.map(|side| side as f32),
                        translate: &|_| None,
                        language: [0; 3],
                        now: 0.0,
                        nodes: &mut painted,
                        next: &mut next,
                        renderers: &mut [],
                    },
                    |_, _| Some(render),
                )
                .unwrap()
                .expect("history label paints");
            let text = painted
                .iter()
                .find(|node| matches!(node.visual(), ui::UiVisual::Text { .. }))
                .expect("retained text exists");
            let origin = text.bounds().min();
            assert_eq!(origin.x(), 10.0 * px);
            assert_eq!(origin.y(), (20.0 + if named { 0.0 } else { 1.0 }) * px);
            let ui::UiVisual::Text { layout, .. } = text.visual() else {
                unreachable!()
            };
            let mut ink = [
                f32::INFINITY,
                f32::INFINITY,
                f32::NEG_INFINITY,
                f32::NEG_INFINITY,
            ];
            for glyph in layout.glyphs() {
                let [x0, y0, x1, y1] = glyph.bounds_64.map(|edge| edge as f32 / 64.0);
                ink = [
                    ink[0].min(x0),
                    ink[1].min(y0),
                    ink[2].max(x1),
                    ink[3].max(y1),
                ];
            }
            let regions = ui_screens::chat_links::hits(
                &[node],
                &ViewState::default(),
                metrics,
                &font,
                &mut presentation.layouts,
                px,
                [0.0; 2],
                &[],
            );
            assert_eq!(regions.len(), 1);
            let bounds = regions[0].1;
            assert_eq!(bounds.min().x(), origin.x() + ink[0]);
            assert_eq!(bounds.min().y(), origin.y() + ink[1]);
            assert_eq!(bounds.max().x(), origin.x() + ink[2]);
            assert_eq!(bounds.max().y(), origin.y() + ink[3]);
        }
    }
}
