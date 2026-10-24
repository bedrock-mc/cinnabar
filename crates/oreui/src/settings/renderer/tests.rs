//! Renderer and pack controls stay in their own feature-gated settings sections.

use crate::test_support::{centre, settings_view};
use launcher::menu::{
    MenuAction, MenuView, RendererChoice,
    settings_sections::SettingSection,
    settings_storage::{StorageAction, StorageFolder},
};
use std::sync::Arc;
use ui_engine::menu_scroll::MenuScrolls;

/// Collects the controls and labels exposed while scrolling the settings content to its end.
fn controls(view: &MenuView) -> (Vec<MenuAction>, Vec<String>) {
    let mut scrolls = MenuScrolls::<MenuAction>::default();
    let (mut seen, mut labels) = (Vec::new(), Vec::new());
    for _ in 0..512 {
        let (areas, hits, nodes) =
            crate::review_tests::paint(scrolls.offsets().clone(), |canvas| {
                crate::settings::draw(
                    canvas,
                    view,
                    [1280.0, 720.0],
                    &|_| None,
                    None,
                    &mut Default::default(),
                )
                .unwrap();
            });
        for (action, _) in hits {
            if !seen.contains(&action) {
                seen.push(action);
            }
        }
        for node in nodes {
            if let ui::UiVisual::Text { layout, .. } = node.visual() {
                labels.push(
                    layout
                        .glyphs()
                        .iter()
                        .map(|glyph| glyph.codepoint)
                        .collect(),
                );
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
    (seen, labels)
}

/// Extracts the choices actually published by the section's painted controls.
fn renderer_choices(view: &MenuView) -> Vec<RendererChoice> {
    controls(view)
        .0
        .into_iter()
        .filter_map(|action| match action {
            MenuAction::SelectRenderer(choice) => Some(choice),
            _ => None,
        })
        .collect()
}

#[test]
fn video_keeps_only_the_renderer_choice_and_shader_shortcut() {
    let mut view = settings_view(SettingSection::Video.definition().name);
    view.shader_packs = Arc::from(["Dusk.zip".to_owned()]);
    view.render_mode = ui::RenderMode::ShaderPack;
    view.shader_pack = Some("Dusk.zip".to_owned());
    view.renderer = Some(RendererChoice::ShaderPack(0, view.shader_packs_revision));
    view.shader_options_available = true;
    Arc::make_mut(&mut view.storage).opens_folders = true;
    let mut expected = Vec::new();
    if render_model::ENHANCED_RENDERING_ENABLED {
        expected.push(RendererChoice::Vanilla);
        expected.push(RendererChoice::Enhanced);
    }
    assert_eq!(renderer_choices(&view), expected);
    let (actions, labels) = controls(&view);
    assert_eq!(
        labels.iter().any(|text| text == "Shaders: Dusk"),
        render_model::SHADER_PACKS_ENABLED
    );
    if render_model::ENHANCED_RENDERING_ENABLED {
        assert!(
            labels
                .iter()
                .any(|text| text.contains("Shader pack active: Dusk"))
        );
    } else {
        assert!(
            !labels
                .iter()
                .any(|text| text == "Vanilla" || text == "Renderer")
        );
    }
    assert!(!actions.contains(&MenuAction::ReloadShaders));
    assert!(
        !actions
            .iter()
            .any(|action| matches!(action, MenuAction::ShaderOptions(_)))
    );
    assert!(
        !actions.contains(&MenuAction::SettingsStorage(StorageAction::OpenFolder(
            StorageFolder::ShaderPacks
        )))
    );
}

#[test]
fn video_shader_shortcut_is_a_labelled_navigation_button() {
    let view = settings_view(SettingSection::Video.definition().name);
    let (actions, labels) = controls(&view);
    assert_eq!(
        actions.contains(&MenuAction::SettingsShaders),
        render_model::SHADER_PACKS_ENABLED
    );
    assert_eq!(
        labels.iter().any(|text| text == "Open"),
        render_model::SHADER_PACKS_ENABLED
    );
    assert!(!labels.iter().any(|text| text == "›"));
}

#[test]
fn video_hides_the_renderer_row_when_only_vanilla_is_available() {
    let mut view = settings_view(SettingSection::Video.definition().name);
    for mode in [ui::RenderMode::Vanilla, ui::RenderMode::ShaderPack] {
        view.render_mode = mode;
        let (actions, labels) = controls(&view);
        assert_eq!(
            actions.contains(&MenuAction::SelectRenderer(RendererChoice::Vanilla)),
            render_model::ENHANCED_RENDERING_ENABLED,
            "{mode:?}"
        );
        assert_eq!(
            labels.iter().any(|text| text == "Renderer"),
            render_model::ENHANCED_RENDERING_ENABLED,
            "{mode:?}"
        );
    }
}

#[test]
fn the_video_shortcut_names_a_selected_pack_missing_from_discovery() {
    let mut view = settings_view(SettingSection::Video.definition().name);
    view.render_mode = ui::RenderMode::ShaderPack;
    view.renderer = None;
    view.shader_pack = Some("Missing.zip".to_owned());
    assert_eq!(
        controls(&view)
            .1
            .iter()
            .any(|text| text == "Shaders: Missing"),
        render_model::SHADER_PACKS_ENABLED
    );
}

#[test]
fn shader_tab_and_video_shortcut_follow_the_shader_pack_feature() {
    let mut view = settings_view(SettingSection::Video.definition().name);
    let (actions, labels) = controls(&view);
    assert_eq!(
        labels.iter().any(|text| text == "Shaders: Off"),
        render_model::SHADER_PACKS_ENABLED
    );
    assert_eq!(
        actions.contains(&MenuAction::SettingsShaders),
        render_model::SHADER_PACKS_ENABLED
    );
    let action = MenuAction::SettingsSection(SettingSection::Shaders.definition().index);
    view.settings.section = SettingSection::Shaders.definition().index;
    let (_, hits, _) = crate::review_tests::paint(Default::default(), |canvas| {
        crate::settings::draw(
            canvas,
            &view,
            [1280.0, 1600.0],
            &|_| None,
            None,
            &mut Default::default(),
        )
        .unwrap();
    });
    assert_eq!(
        hits.iter().any(|(hit, _)| *hit == action),
        render_model::SHADER_PACKS_ENABLED
    );
}

#[test]
fn shaders_offer_off_and_packs_with_the_current_list_revision() {
    let mut view = settings_view(SettingSection::Shaders.definition().name);
    view.shader_packs_revision = 7;
    view.shader_packs = Arc::from(["Dusk.zip".to_owned(), "Lumen".to_owned()]);
    let expected = if render_model::SHADER_PACKS_ENABLED {
        vec![
            RendererChoice::Vanilla,
            RendererChoice::ShaderPack(0, view.shader_packs_revision),
            RendererChoice::ShaderPack(1, view.shader_packs_revision),
        ]
    } else {
        Vec::new()
    };
    assert_eq!(renderer_choices(&view), expected);
}

#[test]
fn shaders_offer_reload_and_folder_import_when_the_host_opens_folders() {
    let open = MenuAction::SettingsStorage(StorageAction::OpenFolder(StorageFolder::ShaderPacks));
    let mut view = settings_view(SettingSection::Shaders.definition().name);
    for opens_folders in [false, true] {
        Arc::make_mut(&mut view.storage).opens_folders = opens_folders;
        let actions = controls(&view).0;
        assert_eq!(
            actions.contains(&open),
            opens_folders && render_model::SHADER_PACKS_ENABLED
        );
        assert!(!actions.contains(&MenuAction::ReloadShaders));
    }
}

#[test]
fn the_import_target_opens_the_picker_with_or_without_file_drops() {
    let mut view = settings_view(SettingSection::Shaders.definition().name);
    for accepts_file_drops in [false, true] {
        Arc::make_mut(&mut view.storage).accepts_file_drops = accepts_file_drops;
        for importing in [false, true] {
            view.shader_pack_importing = importing;
            assert_eq!(
                controls(&view).0.contains(&MenuAction::ImportShaderPacks),
                !importing && render_model::SHADER_PACKS_ENABLED,
                "drops {accepts_file_drops}, importing {importing}"
            );
        }
    }
}

#[test]
fn touch_hosts_offer_a_named_shader_import_button_without_a_folder_shortcut() {
    let mut view = settings_view(SettingSection::Shaders.definition().name);
    let storage = Arc::make_mut(&mut view.storage);
    storage.accepts_file_drops = false;
    storage.opens_folders = false;
    for packs in [Vec::new(), vec!["Dusk.zip".to_owned()]] {
        view.shader_packs = packs.into();
        let (actions, labels) = controls(&view);
        assert_eq!(
            labels.iter().any(|label| label == "Import shader pack"),
            render_model::SHADER_PACKS_ENABLED
        );
        assert_eq!(
            labels.iter().any(|label| label == "Tap to choose a .zip"),
            render_model::SHADER_PACKS_ENABLED
        );
        assert_eq!(
            actions.contains(&MenuAction::ImportShaderPacks),
            render_model::SHADER_PACKS_ENABLED
        );
        assert!(!labels.iter().any(|label| label == "Open folder"));
        assert!(
            !actions.contains(&MenuAction::SettingsStorage(StorageAction::OpenFolder(
                StorageFolder::ShaderPacks,
            )))
        );
    }
}

#[test]
fn a_requested_pack_without_a_picker_entry_offers_its_options() {
    let mut view = settings_view(SettingSection::Shaders.definition().name);
    view.render_mode = ui::RenderMode::ShaderPack;
    view.renderer = None;
    view.shader_options_available = true;
    assert_eq!(
        controls(&view).0.contains(&MenuAction::ShaderOptions(
            launcher::shader_options::Action::Open
        )),
        render_model::SHADER_PACKS_ENABLED
    );
}

#[test]
fn pack_failure_details_appear_only_in_shaders() {
    let mut view = settings_view(SettingSection::Video.definition().name);
    view.shader_packs = Arc::from(["Dusk.zip".to_owned()]);
    view.render_mode = ui::RenderMode::ShaderPack;
    view.shader_pack = Some("Dusk.zip".into());
    view.renderer = Some(RendererChoice::ShaderPack(0, view.shader_packs_revision));
    view.renderer_failure = Some("Pack failed: unknown bloomTint".into());
    assert!(
        !controls(&view)
            .1
            .iter()
            .any(|text| text.contains("unknown bloomTint"))
    );
    view.settings.section = SettingSection::Shaders.definition().index;
    assert_eq!(
        controls(&view)
            .1
            .iter()
            .any(|text| text.contains("unknown bloomTint")),
        render_model::SHADER_PACKS_ENABLED
    );
}

#[test]
fn video_shortcut_shows_off_for_a_remembered_but_disabled_pack() {
    let mut view = settings_view(SettingSection::Video.definition().name);
    view.shader_pack = Some("Dusk.zip".to_owned());
    let labels = controls(&view).1;
    assert!(!labels.iter().any(|text| text == "Shaders: Dusk"));
    assert_eq!(
        labels.iter().any(|text| text == "Shaders: Off"),
        render_model::SHADER_PACKS_ENABLED
    );
}

/// Paints the Shaders tab at `width` and returns every hit target.
fn hits_at(view: &MenuView, width: f32) -> Vec<(MenuAction, ui::UiRect)> {
    crate::review_tests::paint(Default::default(), |canvas| {
        crate::settings::draw(
            canvas,
            view,
            [width, 1600.0],
            &|_| None,
            None,
            &mut Default::default(),
        )
        .unwrap();
    })
    .1
}

/// Paints the Shaders tab once, tall enough to show the whole library, and returns its texts.
fn texts(view: &MenuView) -> Vec<String> {
    crate::review_tests::paint(Default::default(), |canvas| {
        crate::settings::draw(
            canvas,
            view,
            [1280.0, 1600.0],
            &|_| None,
            None,
            &mut Default::default(),
        )
        .unwrap();
    })
    .2
    .into_iter()
    .filter_map(|node| match node.visual() {
        ui::UiVisual::Text { layout, .. } => Some(
            layout
                .glyphs()
                .iter()
                .map(|glyph| glyph.codepoint)
                .collect(),
        ),
        _ => None,
    })
    .collect()
}

fn hit(hits: &[(MenuAction, ui::UiRect)], action: MenuAction) -> ui::UiRect {
    hits.iter()
        .find(|(hit, _)| *hit == action)
        .unwrap_or_else(|| panic!("{action:?} is painted"))
        .1
}

#[test]
fn unselected_shader_pack_rows_stack_without_deletion() {
    if !render_model::SHADER_PACKS_ENABLED {
        return;
    }
    let mut view = settings_view(SettingSection::Shaders.definition().name);
    view.shader_packs = Arc::from(["Dusk.zip".to_owned(), "Lumen.zip".to_owned()]);
    for width in [800.0, 1280.0, 1920.0] {
        let hits = hits_at(&view, width);
        let rows: Vec<_> = [
            RendererChoice::Vanilla,
            RendererChoice::ShaderPack(0, view.shader_packs_revision),
            RendererChoice::ShaderPack(1, view.shader_packs_revision),
        ]
        .map(|choice| hit(&hits, MenuAction::SelectRenderer(choice)))
        .into();
        for pair in rows.windows(2) {
            assert!(pair[0].max().y() <= pair[1].min().y(), "Off leads one list");
            assert_eq!(pair[0].min().x(), pair[1].min().x());
            assert_eq!(pair[0].max().x(), pair[1].max().x());
        }
        assert!(
            !hits
                .iter()
                .any(|(action, _)| matches!(action, MenuAction::RemoveShaderPackDialog(..))),
            "only the selected pack offers deletion"
        );
    }
}

#[test]
fn the_selected_pack_card_holds_its_controls_apart_from_selection() {
    if !render_model::SHADER_PACKS_ENABLED {
        return;
    }
    let mut view = settings_view(SettingSection::Shaders.definition().name);
    view.shader_packs = Arc::from(["Dusk.zip".to_owned(), "Lumen.zip".to_owned()]);
    view.render_mode = ui::RenderMode::ShaderPack;
    view.shader_pack = Some("Dusk.zip".into());
    view.renderer = Some(RendererChoice::ShaderPack(0, view.shader_packs_revision));
    view.shader_options_available = true;
    for width in [800.0, 1280.0, 1920.0] {
        let hits = hits_at(&view, width);
        let select = hit(&hits, MenuAction::SelectRenderer(view.renderer.unwrap()));
        let next = hit(
            &hits,
            MenuAction::SelectRenderer(RendererChoice::ShaderPack(1, view.shader_packs_revision)),
        );
        let remove = hit(
            &hits,
            MenuAction::RemoveShaderPackDialog(0, view.shader_packs_revision),
        );
        assert!(
            select.max().x() < remove.min().x(),
            "delete sits right of selection"
        );
        assert!(remove.min().y() >= select.min().y() && remove.max().y() <= select.max().y());
        assert!(
            remove.max().x() <= next.max().x(),
            "delete stays inside the card"
        );
        for control in [
            MenuAction::ShaderOptions(launcher::shader_options::Action::Open),
            MenuAction::ReloadShaders,
        ] {
            let control = hit(&hits, control);
            assert!(
                control.min().y() >= select.max().y(),
                "{width}: below the pack"
            );
            assert!(
                control.max().y() <= next.min().y(),
                "{width}: before the next pack"
            );
        }
        assert_eq!(
            hits.iter()
                .filter(|(action, _)| matches!(action, MenuAction::RemoveShaderPackDialog(..)))
                .count(),
            1
        );
    }
}

#[test]
fn the_selected_pack_shows_its_load_state_instead_of_its_file_name() {
    if !render_model::SHADER_PACKS_ENABLED {
        return;
    }
    let mut view = settings_view(SettingSection::Shaders.definition().name);
    view.shader_packs = Arc::from(["Dusk.zip".to_owned(), "Lumen".to_owned()]);
    view.render_mode = ui::RenderMode::ShaderPack;
    view.shader_pack = Some("Dusk.zip".into());
    view.renderer = Some(RendererChoice::ShaderPack(0, view.shader_packs_revision));
    let labels = texts;
    let shown = labels(&view);
    assert!(shown.iter().any(|text| text == "Active"));
    assert!(!shown.iter().any(|text| text.contains("Dusk.zip")));
    assert_eq!(shown.iter().filter(|text| *text == "Lumen").count(), 1);
    view.shader_pack_loading = true;
    assert!(labels(&view).iter().any(|text| text == "Loading…"));
    view.renderer_failure = Some("Pack failed: unknown bloomTint".into());
    let shown = labels(&view);
    assert!(shown.iter().any(|text| text == "Failed"));
    assert!(shown.iter().any(|text| text.contains("unknown bloomTint")));
    view.shader_pack_removing = Some(Arc::from("Dusk.zip"));
    assert!(labels(&view).iter().any(|text| text == "Removing…"));
}

#[test]
fn only_packs_sharing_a_name_show_their_kind() {
    if !render_model::SHADER_PACKS_ENABLED {
        return;
    }
    let mut view = settings_view(SettingSection::Shaders.definition().name);
    view.shader_packs = Arc::from(["Dusk.zip".to_owned(), "Lumen".to_owned()]);
    let kinds = |view: &MenuView| {
        texts(view)
            .into_iter()
            .filter(|text| text == "ZIP" || text == "Folder")
            .collect::<Vec<_>>()
    };
    assert!(kinds(&view).is_empty());
    view.shader_packs = Arc::from(["Dusk.zip".to_owned(), "Dusk".to_owned()]);
    assert_eq!(kinds(&view), ["ZIP", "Folder"]);
}

#[test]
fn long_shader_libraries_remain_scrollable_and_selectable() {
    if !render_model::SHADER_PACKS_ENABLED {
        return;
    }
    let mut view = settings_view(SettingSection::Shaders.definition().name);
    view.shader_packs = (0..24)
        .map(|i| format!("Shader pack {i} with a long descriptive release name.zip"))
        .collect();
    assert_eq!(renderer_choices(&view).len(), view.shader_packs.len() + 1);
}

#[test]
fn removal_is_available_only_for_managed_entries_and_locks_while_pending() {
    if !render_model::SHADER_PACKS_ENABLED {
        return;
    }
    let mut view = settings_view(SettingSection::Shaders.definition().name);
    view.shader_packs = Arc::from(["Dusk.zip".to_owned()]);
    view.render_mode = ui::RenderMode::ShaderPack;
    view.shader_pack = Some("Dusk.zip".into());
    view.renderer = Some(RendererChoice::ShaderPack(0, view.shader_packs_revision));
    view.shader_options_available = true;
    let options = MenuAction::ShaderOptions(launcher::shader_options::Action::Open);
    let remove = MenuAction::RemoveShaderPackDialog(0, view.shader_packs_revision);
    assert!(controls(&view).0.contains(&remove));
    assert!(controls(&view).0.contains(&options));
    view.shader_pack_removing = Some(Arc::from("Dusk.zip"));
    assert!(!controls(&view).0.contains(&remove));
    assert!(!controls(&view).0.contains(&options));
    assert!(!controls(&view).0.contains(&MenuAction::ReloadShaders));
    view.shader_pack_removing = None;
    view.shader_packs = Arc::from([]);
    view.render_mode = ui::RenderMode::ShaderPack;
    view.renderer = None;
    view.shader_pack = Some("Outside.zip".into());
    assert!(
        !controls(&view)
            .0
            .iter()
            .any(|action| matches!(action, MenuAction::RemoveShaderPackDialog(..)))
    );
}
