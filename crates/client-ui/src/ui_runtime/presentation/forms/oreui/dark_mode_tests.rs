use super::*;
use crate::menu::settings_options::{OREUI_DARK_MODE, SETTINGS_OPTIONS};
use crate::ui_runtime::presentation::tests::fixture_font;

#[test]
fn dark_mode_dims_the_full_home_backdrop_more_than_default() {
    let view = MenuView::new(true, "Player".into());
    let overlay = |appearance| {
        let (_, _, nodes) = review_tests::paint(Default::default(), |canvas| {
            canvas.appearance = appearance;
            home::draw(canvas, &view, [1280.0, 720.0], None, &|_| None).unwrap();
        });
        review_tests::solids(&nodes)
            .into_iter()
            .find_map(|(bounds, color)| {
                (bounds == [0.0, 0.0, 1280.0, 720.0] && color[..3] == [0; 3]).then_some(color[3])
            })
            .unwrap()
    };
    assert!(overlay(theme::Appearance::Dark) > overlay(theme::Appearance::Default));
}

#[test]
fn home_quit_confirmation_uses_oreui_without_a_legacy_popup_and_traps_clicks() {
    let mut runtime = UiPresentationRuntime::new(fixture_font()).unwrap();
    let mut view = MenuView::new(true, "Player".into());
    view.dialog = Some(crate::menu::MenuDialog::Exit);
    set_dark(&mut view, true);
    runtime.set_menu_view(Some(view));
    runtime
        .form_presentation
        .oreui_transitions
        .configure_motion(false);
    let metrics = TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2));
    let (mut nodes, mut next) = (Vec::new(), 1);
    let hits = runtime
        .append_menu(
            &crate::ui_runtime::UiRuntime::new(1),
            &mut nodes,
            &mut next,
            metrics,
            1280.0,
            720.0,
        )
        .unwrap();
    assert!(
        hits.iter()
            .any(|(action, _)| *action == MenuAction::ConfirmExit)
    );
    assert!(
        hits.iter()
            .any(|(action, _)| *action == MenuAction::DismissDialog)
    );
    assert!(
        hits.iter().all(|(action, _)| matches!(
            action,
            MenuAction::ConfirmExit | MenuAction::DismissDialog
        ))
    );
    assert!(
        review_tests::solids(&nodes)
            .iter()
            .any(|(_, color)| *color == theme::Appearance::Dark.role(theme::SECONDARY).fill)
    );
}

#[test]
fn dark_mode_preserves_full_color_art_and_readable_labels_on_light_tags() {
    let (_, _, nodes) = review_tests::paint(Default::default(), |canvas| {
        canvas.appearance = theme::Appearance::Dark;
        widgets::tag(
            canvas,
            "Owner",
            [0.0, 0.0],
            theme::SUCCESS_TINT,
            theme::TEXT_DARK,
        )
        .unwrap();
        canvas
            .icon_ref(
                super::super::super::IconRef {
                    page: 17,
                    uv: [0, 0, 32, 32],
                    glint: false,
                },
                [0.0, 30.0, 32.0, 62.0],
            )
            .unwrap();
    });
    assert!(nodes.iter().any(|node| matches!(node.visual(), ui::UiVisual::Text { color, .. } if *color == theme::TEXT_DARK)));
    assert!(nodes.iter().any(|node| matches!(
        node.visual(),
        ui::UiVisual::Sprite {
            texture_page: 17,
            color: [255, 255, 255, 255],
            ..
        }
    )));
}

fn set_dark(view: &mut MenuView, enabled: bool) {
    let index = SETTINGS_OPTIONS
        .iter()
        .position(|option| option.name == OREUI_DARK_MODE)
        .unwrap();
    Arc::make_mut(&mut view.settings_options).set(index, i32::from(enabled));
}

fn draw(
    runtime: &mut UiPresentationRuntime,
    view: &MenuView,
) -> (Vec<UiNode>, Vec<(MenuAction, UiRect)>) {
    let metrics = TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2));
    let (mut nodes, mut next) = (Vec::new(), 1);
    let hits = runtime
        .append_oreui_screen(
            view,
            &mut nodes,
            &mut next,
            metrics,
            [1280.0, 720.0],
            None,
            &|_| None,
        )
        .unwrap()
        .unwrap();
    runtime.end_animation_frame();
    (nodes, hits)
}

#[test]
fn dark_mode_applies_immediately_and_restores_the_default_without_changing_inputs() {
    for screen in [
        MenuScreen::Home,
        MenuScreen::Settings,
        MenuScreen::Servers,
        MenuScreen::Inbox,
        MenuScreen::AddServer,
        MenuScreen::Pause,
    ] {
        let mut runtime = UiPresentationRuntime::new(fixture_font()).unwrap();
        let mut view = MenuView::new(true, "Player".into());
        view.screen = screen;
        // An appearance change must not depend on the motion setting.
        let motion = SETTINGS_OPTIONS
            .iter()
            .position(|option| option.name == "screen_animations")
            .unwrap();
        Arc::make_mut(&mut view.settings_options).set(motion, 0);
        runtime
            .form_presentation
            .oreui_transitions
            .configure_motion(false);
        let (before, hits) = draw(&mut runtime, &view);
        set_dark(&mut view, true);
        let (dark, dark_hits) = draw(&mut runtime, &view);
        assert_eq!(hits, dark_hits, "{screen:?}");
        assert_eq!(
            before.iter().map(|n| n.bounds()).collect::<Vec<_>>(),
            dark.iter().map(|n| n.bounds()).collect::<Vec<_>>(),
            "{screen:?}"
        );
        assert_ne!(
            super::review_tests::solids(&before),
            super::review_tests::solids(&dark),
            "{screen:?}"
        );
        for node in &dark {
            if let ui::UiVisual::Text { color, .. } = node.visual() {
                assert_ne!(*color, theme::TEXT_DARK, "dark text in {screen:?}");
            }
        }
        set_dark(&mut view, false);
        let (restored, restored_hits) = draw(&mut runtime, &view);
        assert_eq!(restored_hits, hits);
        assert_eq!(
            super::review_tests::solids(&restored),
            super::review_tests::solids(&before),
            "{screen:?}"
        );
    }
}

#[test]
fn dark_mode_survives_a_frame_reset_and_loading_without_a_menu_view() {
    let mut runtime = UiPresentationRuntime::new(fixture_font()).unwrap();
    let mut view = MenuView::new(true, "Player".into());
    set_dark(&mut view, true);
    draw(&mut runtime, &view);
    runtime
        .form_presentation
        .oreui_transitions
        .configure_motion(false);
    runtime.begin_form_frame();
    let metrics = TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2));
    let (mut nodes, mut next) = (Vec::new(), 1);
    runtime
        .append_oreui_loading(
            super::super::loading_screen::LoadingStage::BuildingTerrain,
            ["Generating world", "Building terrain"],
            &mut nodes,
            &mut next,
            metrics,
            [1280.0, 720.0],
        )
        .unwrap();
    assert!(
        super::review_tests::solids(&nodes)
            .iter()
            .any(|(_, color)| *color == theme::Appearance::Dark.role(theme::NEUTRAL80).fill)
    );
    assert!(
        !super::review_tests::solids(&nodes)
            .iter()
            .any(|(_, color)| *color == theme::NEUTRAL80.fill)
    );
}

#[test]
fn dark_mode_setting_has_a_label_description_and_reachable_toggle() {
    let mut runtime = UiPresentationRuntime::new(fixture_font()).unwrap();
    let mut view = MenuView::new(true, "Player".into());
    view.screen = MenuScreen::Settings;
    view.settings_section = super::super::menu_screens::SETTINGS_SECTIONS
        .iter()
        .find_map(|(name, index)| (*name == "video_forced_index").then_some(*index))
        .unwrap();
    set_dark(&mut view, true);
    draw(&mut runtime, &view);
    let motion = SETTINGS_OPTIONS
        .iter()
        .position(|option| option.name == "screen_animations")
        .unwrap();
    Arc::make_mut(&mut view.settings_options).set(motion, 0);
    let option = SETTINGS_OPTIONS
        .iter()
        .position(|option| option.name == OREUI_DARK_MODE)
        .unwrap() as u16;
    let (nodes, hits) = (0..80)
        .find_map(|_| {
            let frame = draw(&mut runtime, &view);
            if frame
                .1
                .iter()
                .any(|(action, _)| *action == MenuAction::SettingsOption(option, 0))
            {
                Some(frame)
            } else {
                runtime.scroll_menu(UiPoint::new(1000.0, 500.0).unwrap(), -2.0, false);
                None
            }
        })
        .expect("the dark mode toggle must be reachable by scrolling");
    assert!(
        hits.iter()
            .any(|(action, _)| *action == MenuAction::SettingsOption(option, 0))
    );
    let text = nodes
        .iter()
        .filter_map(|node| match node.visual() {
            ui::UiVisual::Text { layout, .. } => Some(
                layout
                    .glyphs()
                    .iter()
                    .map(|glyph| glyph.codepoint)
                    .collect::<String>(),
            ),
            _ => None,
        })
        .collect::<String>();
    assert!(text.replace(' ', "").contains("DarkMode"));
    assert!(text.replace(' ', "").contains("Usedarksurfaces"));
}
