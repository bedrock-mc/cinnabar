//! The Video renderer picker publishes only the renderers this build compiles.

use super::pack_label;
use crate::test_support::{centre, settings_view};
use launcher::menu::{MenuAction, MenuView, RendererChoice};
use std::sync::Arc;
use ui_engine::menu_scroll::MenuScrolls;

/// Every renderer choice the Video pane publishes while scrolling to its end.
fn renderer_choices(view: &MenuView) -> Vec<RendererChoice> {
    let size = [1280.0, 720.0];
    let mut scrolls = MenuScrolls::<MenuAction>::default();
    let mut seen = Vec::new();
    for _ in 0..512 {
        let (areas, hits, _) = crate::review_tests::paint(scrolls.offsets().clone(), |canvas| {
            crate::settings::draw(canvas, view, size, &|_| None, None).unwrap();
        });
        for (action, _) in &hits {
            if let MenuAction::SelectRenderer(choice) = action
                && !seen.contains(choice)
            {
                seen.push(*choice);
            }
        }
        let viewport = areas
            .iter()
            .max_by(|left, right| left.viewport.min().x().total_cmp(&right.viewport.min().x()))
            .expect("settings content scroll view")
            .viewport;
        scrolls.set_areas(areas);
        let before = scrolls.offsets().clone();
        scrolls.wheel(centre(viewport), -1.0, false);
        if *scrolls.offsets() == before {
            break;
        }
    }
    seen
}

#[test]
fn renderer_choices_follow_the_compiled_features() {
    let mut view = settings_view("video_forced_index");
    view.shader_packs = Arc::from(["Dusk.zip".to_owned(), "Lumen".to_owned()]);
    let renderers = renderer_choices(&view);
    let mut expected = Vec::new();
    if render_model::ENHANCED_RENDERING_ENABLED || render_model::SHADER_PACKS_ENABLED {
        expected.push(RendererChoice::Vanilla);
    }
    if render_model::ENHANCED_RENDERING_ENABLED {
        expected.push(RendererChoice::Enhanced);
    }
    if render_model::SHADER_PACKS_ENABLED {
        expected.extend([RendererChoice::ShaderPack(0), RendererChoice::ShaderPack(1)]);
    }
    assert_eq!(renderers.len(), expected.len(), "{renderers:?}");
    assert!(expected.iter().all(|choice| renderers.contains(choice)));
}

#[test]
fn pack_labels_drop_only_a_zip_extension() {
    assert_eq!(pack_label("Dusk.zip"), "Dusk");
    assert_eq!(pack_label("Dusk.ZIP"), "Dusk");
    assert_eq!(pack_label("Lumen"), "Lumen");
    assert_eq!(pack_label(".zip"), ".zip");
    assert_eq!(pack_label("Lumen v2.zipped"), "Lumen v2.zipped");
}
