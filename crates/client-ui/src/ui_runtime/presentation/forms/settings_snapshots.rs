//! Offline settings gallery through the installed vanilla UI carrier.

use crate::menu::MenuScreen;

struct Metrics;

impl json_ui::TextMeasure for Metrics {
    /// Stable metrics isolate selector geometry from the installed font.
    fn extent(&self, text: &str) -> [f64; 2] {
        [text.len() as f64 * 6.0, 9.0]
    }
}

impl json_ui::TextureSource for Metrics {
    /// Selector dimensions are explicit and do not depend on texture dimensions.
    fn texture(&self, _path: &str) -> Option<json_ui::TextureMeta> {
        None
    }
}

/// Find a resolved selector in the layout, including controls below the viewport.
fn selector(tree: &json_ui::LaidOut<'_>, name: &str) -> Option<json_ui::Rect> {
    if tree.control.name == name {
        return Some(tree.rect);
    }
    tree.children.iter().find_map(|child| selector(child, name))
}

#[test]
fn settings_category_button_pitch_matches_vanilla_toggle_height() {
    let Some(carrier) = super::pack_harness::carrier() else {
        eprintln!(
            "skipping settings_category_button_pitch_matches_vanilla_toggle_height: fixture unavailable; requires installed UI carrier (make assets)"
        );
        return;
    };
    let files = carrier.ui_files();
    let catalog =
        json_ui::Catalog::from_files(files.iter().map(|file| (&*file.path, &*file.bytes))).unwrap();
    let (reference, context) = super::menu_screens::settings_target();
    let tree = json_ui::resolve(&catalog, reference, &context)
        .control
        .unwrap();
    let layout = json_ui::layout(
        &tree,
        [640.0, 360.0],
        &json_ui::LayoutEnv {
            text: &Metrics,
            textures: &Metrics,
        },
    );
    let general = selector(&layout, "general_button").unwrap();
    let video = selector(&layout, "video_button").unwrap();
    let audio = selector(&layout, "sound_button").unwrap();
    // settings_common.section_toggle_base is 30px; its 31px background overlaps the seam.
    assert_eq!(general.h, 30.0);
    assert_eq!(video.y - general.y, general.h);
    assert_eq!(audio.y - video.y, video.h);
}

/// Render one desktop section selected by its controller variable.
fn section(variable: &str) {
    section_with(variable, &format!("settings-{variable}"), |_| {});
}

/// Render a section after adjusting the menu view, such as opening a dropdown.
fn section_with(variable: &str, name: &str, adjust: impl FnOnce(&mut crate::menu::MenuView)) {
    if super::pack_harness::carrier().is_none() {
        return;
    }
    let player_runtime = player_state::PlayerState::new(1);
    let mut view = crate::menu::MenuView::new(true, "Steve".to_owned());
    let local = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("../.local");
    view.language_choices =
        crate::menu::settings_options::SettingsOptions::language_choices(&local);
    view.screen = MenuScreen::Settings;
    view.settings_section = super::menu_screens::SETTINGS_SECTIONS
        .iter()
        .find_map(|(name, index)| (*name == variable).then_some(*index))
        .expect("known settings section");
    adjust(&mut view);
    super::play_flow_snapshots::snapshot(&player_runtime, &view, name);
}

#[test]
fn settings_sections_gallery() {
    for section_name in [
        "accessibility_forced_index",
        "keyboard_and_mouse_forced_index",
        "controller_and_switch_forced_index",
        "general_forced_index",
        "account_forced_index",
        "video_forced_index",
        "sound_forced_index",
        "creator_forced_index",
        "global_texture_pack_forced_index",
        "language_forced_index",
    ] {
        section(section_name);
    }
}

#[test]
fn settings_video_graphics_mode_open() {
    let index = crate::menu::settings_options::SETTINGS_OPTIONS
        .iter()
        .position(|option| option.name == "graphics_mode")
        .unwrap();
    section_with(
        "video_forced_index",
        "settings-video-graphics-open",
        |view| view.settings_dropdown = Some(index as u16),
    );
}

// Every row of an open settings dropdown is its own click target for its own choice.
#[test]
fn open_graphics_mode_rows_select_their_own_choice() {
    let player_runtime = player_state::PlayerState::new(1);

    let Some(mut presentation) = super::pack_harness::engine_presentation() else {
        eprintln!(
            "skipping open_graphics_mode_rows_select_their_own_choice: fixture unavailable; requires installed UI carrier (make assets)"
        );
        return;
    };
    let index = crate::menu::settings_options::SETTINGS_OPTIONS
        .iter()
        .position(|option| option.name == "graphics_mode")
        .unwrap() as u16;
    let mut view = crate::menu::MenuView::new(true, "Steve".to_owned());
    view.screen = MenuScreen::Settings;
    view.settings_section = super::menu_screens::SETTINGS_SECTIONS
        .iter()
        .find_map(|(name, at)| (*name == "video_forced_index").then_some(*at))
        .unwrap();
    view.settings_dropdown = Some(index);
    presentation.set_menu_view(Some(view));
    presentation
        .build(
            &player_runtime,
            &crate::ui_runtime::UiRuntime::new(1),
            0,
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    for choice in 0..2 {
        let action = crate::menu::MenuAction::SettingsOption(index, choice);
        let row = presentation
            .menu_hit_targets
            .iter()
            .find_map(|(target, bounds)| (*target == action).then_some(*bounds))
            .expect("a row for each choice");
        let (min, max) = (row.min(), row.max());
        let centre =
            ui::UiPoint::new((min.x() + max.x()) / 2.0, (min.y() + max.y()) / 2.0).unwrap();
        assert_eq!(presentation.hit_test_menu(centre), Some(action));
    }
}

#[test]
fn settings_video_graphics_options_pack_before() {
    if super::pack_harness::carrier().is_none() {
        return;
    }
    let player_runtime = player_state::PlayerState::new(1);

    let mut view = crate::menu::MenuView::new(true, "Steve".to_owned());
    view.screen = MenuScreen::Settings;
    view.settings_section = super::menu_screens::SETTINGS_SECTIONS
        .iter()
        .find_map(|(name, index)| (*name == "video_forced_index").then_some(*index))
        .unwrap();
    simple_graphics(&mut view);
    view.settings_advanced_graphics = true;
    super::play_flow_snapshots::snapshot_vanilla(
        &player_runtime,
        &view,
        "settings-video-pack-before",
    );
}

#[test]
fn settings_video_graphics_options_expanded() {
    section_with("video_forced_index", "settings-video-expanded", |view| {
        simple_graphics(view);
        view.settings_advanced_graphics = true;
    });
}

/// Selects the registry's Simple graphics choice for the requested screenshot state.
fn simple_graphics(view: &mut crate::menu::MenuView) {
    let index = crate::menu::settings_options::SETTINGS_OPTIONS
        .iter()
        .position(|option| option.name == "graphics_mode")
        .unwrap();
    std::sync::Arc::make_mut(&mut view.settings_options).set(index, 0);
}

// A complete section-button hit target expands and collapses the original option grid.
#[test]
fn graphics_options_expander_uses_full_settings_button_height() {
    let Some(carrier) = super::pack_harness::carrier() else {
        eprintln!(
            "skipping graphics_options_expander_uses_full_settings_button_height: fixture unavailable; requires installed UI carrier (make assets)"
        );
        return;
    };
    let files = carrier.ui_files();
    let mut catalog =
        json_ui::Catalog::from_files(files.iter().map(|file| (&*file.path, &*file.bytes))).unwrap();
    super::graphics_expander::install(&mut catalog);
    let mut view = crate::menu::MenuView::new(true, "Steve".to_owned());
    view.screen = MenuScreen::Settings;
    view.settings_section = super::menu_screens::SETTINGS_SECTIONS
        .iter()
        .find_map(|(name, index)| (*name == "video_forced_index").then_some(*index))
        .unwrap();
    for expanded in [false, true, false] {
        view.settings_advanced_graphics = expanded;
        let screen = super::menu_screens::screen_data(&view, &|_| None).unwrap();
        let rendered = json_ui::render_screen(
            screen.reference,
            &catalog,
            &screen.context,
            &screen.data,
            [640.0, 360.0],
            &json_ui::LayoutEnv {
                text: &Metrics,
                textures: &Metrics,
            },
            &json_ui::ViewState::default(),
        )
        .unwrap();
        let hit = rendered
            .hits
            .iter()
            .find(|hit| hit.pressed.as_deref() == Some("button.expand_advanced_graphics"))
            .unwrap();
        assert_eq!(hit.rect.h, 30.0);
        assert_eq!(
            super::settings_controls::action(&view, hit),
            Some(crate::menu::MenuAction::SettingsAdvancedGraphics)
        );
        let texture = if expanded {
            "textures/ui/arrowDown"
        } else {
            "textures/ui/arrowRight"
        };
        assert!(rendered.nodes.iter().any(|node| matches!(&node.draw, json_ui::Draw::Sprite { texture: found, .. } if found == texture)));
    }
}

/// Vanilla toggles remain reachable in document order, including scroll content.
#[test]
fn video_toggles_admit_pointer_and_keyboard_focus() {
    let Some(mut presentation) = super::pack_harness::engine_presentation() else {
        eprintln!(
            "skipping video_toggles_admit_pointer_and_keyboard_focus: missing installed UI carrier; make assets"
        );
        return;
    };
    let mut view = crate::menu::MenuView::new(true, "Player".to_owned());
    view.screen = MenuScreen::Settings;
    view.settings_section = super::menu_screens::SETTINGS_SECTIONS
        .iter()
        .find_map(|(name, index)| (*name == "video_forced_index").then_some(*index))
        .unwrap();
    let player = player_state::PlayerState::new(1);
    super::test_support::draw_menu_actions(&player, &mut presentation, &view);
    let actions = presentation.menu_focus_actions().collect::<Vec<_>>();
    let mut previous = None;
    for name in ["hide_hand", "view_bobbing"] {
        let index = crate::menu::settings_options::SETTINGS_OPTIONS
            .iter()
            .position(|option| option.name == name)
            .unwrap() as u16;
        let action = actions.iter().find(|action| matches!(action, crate::menu::MenuAction::SettingsOption(at, _) if *at == index))
            .copied().unwrap_or_else(|| panic!("missing focus action for {name}"));
        let position = actions
            .iter()
            .position(|candidate| *candidate == action)
            .unwrap();
        assert!(
            previous.is_none_or(|previous| previous < position),
            "toggle focus does not follow the pack's control order"
        );
        previous = Some(position);
        view.focused_action = Some(action);
        let hits = super::test_support::draw_menu_actions(&player, &mut presentation, &view);
        assert!(
            hits.contains(&action),
            "focused toggle {name} did not scroll into view"
        );
    }
}

/// Both responsive selector forms accept pointer and keyboard/controller focus.
#[test]
fn animations_selector_and_choices_admit_pointer_and_navigation_focus() {
    use crate::menu::{
        MenuAction,
        settings_options::{ANIMATIONS_OPTION, SETTINGS_OPTIONS},
    };
    let Some(mut presentation) = super::pack_harness::engine_presentation() else {
        eprintln!(
            "skipping animations_selector_and_choices_admit_pointer_and_navigation_focus: missing installed UI carrier; make assets"
        );
        return;
    };
    let index = SETTINGS_OPTIONS
        .iter()
        .position(|option| option.name == ANIMATIONS_OPTION.name)
        .expect("animations selector") as u16;
    let mut view = crate::menu::MenuView::new(true, "Player".to_owned());
    view.screen = MenuScreen::Settings;
    view.settings_section = super::menu_screens::SETTINGS_SECTIONS
        .iter()
        .find_map(|(name, index)| (*name == "video_forced_index").then_some(*index))
        .unwrap();
    let player = player_state::PlayerState::new(1);
    let runtime = crate::ui_runtime::UiRuntime::new(1);
    let draw = |presentation: &mut super::super::UiPresentationRuntime,
                view: &crate::menu::MenuView,
                size| {
        for _ in 0..2 {
            presentation.set_menu_view(Some(view.clone()));
            presentation
                .build(&player, &runtime, 0, size, ui::DpiScale::new(1.0).unwrap())
                .unwrap();
        }
    };
    let dropdown = MenuAction::SettingsDropdown(index);
    for (size, picker) in [([1280, 720], false), ([200, 720], true)] {
        view.settings_dropdown = None;
        view.focused_action = None;
        draw(&mut presentation, &view, size);
        if picker {
            assert!(
                presentation
                    .menu_focus_actions()
                    .any(|action| action == dropdown)
            );
            view.focused_action = Some(dropdown);
            draw(&mut presentation, &view, size);
            let bounds = presentation
                .menu_hit_targets
                .iter()
                .find_map(|(action, bounds)| (*action == dropdown).then_some(*bounds))
                .expect("focused compact selector scrolls into view");
            let center = ui::UiPoint::new(
                (bounds.min().x() + bounds.max().x()) * 0.5,
                (bounds.min().y() + bounds.max().y()) * 0.5,
            )
            .unwrap();
            assert_eq!(presentation.hit_test_menu(center), Some(dropdown));
            view.settings_dropdown = Some(index);
        }
        for choice in ANIMATIONS_OPTION.min..=ANIMATIONS_OPTION.max {
            let action = MenuAction::SettingsOption(index, choice);
            draw(&mut presentation, &view, size);
            assert!(
                presentation
                    .menu_focus_actions()
                    .any(|candidate| candidate == action),
                "selector choice {choice} must be reachable by keyboard/controller navigation",
            );
            view.focused_action = Some(action);
            draw(&mut presentation, &view, size);
            let bounds = presentation
                .menu_hit_targets
                .iter()
                .find_map(|(candidate, bounds)| (*candidate == action).then_some(*bounds))
                .expect("focused animation choice scrolls into view");
            let center = ui::UiPoint::new(
                (bounds.min().x() + bounds.max().x()) * 0.5,
                (bounds.min().y() + bounds.max().y()) * 0.5,
            )
            .unwrap();
            assert_eq!(presentation.hit_test_menu(center), Some(action));
        }
    }
}
