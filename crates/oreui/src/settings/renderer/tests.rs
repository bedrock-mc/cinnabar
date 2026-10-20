//! The Video renderer picker publishes only the renderers this build compiles.

use super::pack_label;
use crate::test_support::{centre, settings_view};
use launcher::menu::{
    MenuAction, MenuView, RendererChoice,
    settings_storage::{StorageAction, StorageFolder},
};
use std::sync::Arc;
use ui_engine::menu_scroll::MenuScrolls;

/// Every action the Video pane publishes while scrolling to its end, in first-seen order.
fn video_actions(view: &MenuView) -> Vec<MenuAction> {
    let size = [1280.0, 720.0];
    let mut scrolls = MenuScrolls::<MenuAction>::default();
    let mut seen = Vec::new();
    for _ in 0..512 {
        let (areas, hits, _) = crate::review_tests::paint(scrolls.offsets().clone(), |canvas| {
            crate::settings::draw(canvas, view, size, &|_| None, None).unwrap();
        });
        for (action, _) in hits {
            if !seen.contains(&action) {
                seen.push(action);
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

/// Every renderer choice the Video pane publishes while scrolling to its end.
fn renderer_choices(view: &MenuView) -> Vec<RendererChoice> {
    video_actions(view)
        .into_iter()
        .filter_map(|action| match action {
            MenuAction::SelectRenderer(choice) => Some(choice),
            _ => None,
        })
        .collect()
}

#[test]
fn shader_pack_builds_offer_the_pack_folder_when_the_host_opens_folders() {
    let open = MenuAction::SettingsStorage(StorageAction::OpenFolder(StorageFolder::ShaderPacks));
    let mut view = settings_view("video_forced_index");
    for opens_folders in [false, true] {
        Arc::make_mut(&mut view.storage).opens_folders = opens_folders;
        assert_eq!(
            video_actions(&view).contains(&open),
            opens_folders && render_model::SHADER_PACKS_ENABLED,
            "opens folders: {opens_folders}"
        );
    }
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
        expected.extend([
            RendererChoice::ShaderPack(0, view.shader_packs_revision),
            RendererChoice::ShaderPack(1, view.shader_packs_revision),
        ]);
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
