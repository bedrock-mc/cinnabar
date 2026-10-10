use crate::ui_runtime::presentation::tests::fixture_font;
use launcher::menu::MenuField;
use {
    super::*,
    launcher::menu::auth::AuthState,
    launcher::menu::{MenuAction, MenuScreen, MenuView},
};

fn append(
    presentation: &mut UiPresentationRuntime,
    view: &MenuView,
    size: [f32; 2],
) -> Vec<(MenuAction, UiRect)> {
    let metrics = TextMetrics::for_viewport(
        size.map(|value| value as u32),
        ui::DpiScale::new(1.0).unwrap(),
        Some(2),
    );
    presentation
        .append_oreui_screen(view, &mut Vec::new(), &mut 1, metrics, size, None, &|_| {
            None
        })
        .unwrap()
        .expect("the route owns its OreUI presentation")
}

#[test]
fn death_owns_oreui_actions_and_preserves_literal_reason() {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let mut view = MenuView::new(true, "Player".into());
    view.screen = MenuScreen::Death;
    view.death_reason = "Player fell with 100% luck %entity.zombie.name".into();
    let metrics = TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2));
    let mut nodes = Vec::new();
    let route = presentation
        .append_oreui_screen(
            &view,
            &mut nodes,
            &mut 1,
            metrics,
            [1280.0, 720.0],
            None,
            &|_| None,
        )
        .unwrap();
    let hits = route.expect("death owns the modern OreUI route");
    assert_eq!(
        hits.iter().map(|(action, _)| *action).collect::<Vec<_>>(),
        vec![MenuAction::Respawn, MenuAction::OpenDeathGameMenu]
    );
    let text: Vec<_> = nodes
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
        .collect();
    let visible_reason: String = view
        .death_reason
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    assert!(
        text.iter().any(|value| value
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            == visible_reason),
        "{text:?}"
    );
}

#[test]
fn death_long_reasons_keep_actions_inside_the_viewport() {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let mut view = MenuView::new(true, "Player".into());
    view.screen = MenuScreen::Death;
    view.death_reason = "A server-authored reason that wraps into several lines.\n".repeat(100);
    for size in [[1280.0, 720.0], [640.0, 360.0]] {
        let hits = append(&mut presentation, &view, size);
        for action in [MenuAction::Respawn, MenuAction::OpenDeathGameMenu] {
            let bounds = hits.iter().find(|(found, _)| *found == action).unwrap().1;
            assert!(bounds.min().y() >= 0.0);
            assert!(
                bounds.max().y() <= size[1],
                "{action:?} must remain reachable"
            );
        }
    }
}

#[test]
fn death_multiline_reasons_preserve_the_modern_route_and_hardcore_actions() {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let mut view = MenuView::new(true, "Player".into());
    view.screen = MenuScreen::Death;
    view.death_reason = "x\n".repeat(ui::MAX_WRAP_LINES);
    for hardcore in [false, true] {
        view.death_presentation.hardcore = hardcore;
        for size in [[1280.0, 720.0], [640.0, 360.0]] {
            let hits = append(&mut presentation, &view, size);
            let primary = if hardcore {
                MenuAction::DeathExitWorld
            } else {
                MenuAction::Respawn
            };
            let secondary = if hardcore {
                MenuAction::Respawn
            } else {
                MenuAction::OpenDeathGameMenu
            };
            assert_eq!(
                hits.iter().map(|(action, _)| *action).collect::<Vec<_>>(),
                [primary, secondary]
            );
            assert!(
                hits.iter()
                    .all(|(_, bounds)| bounds.min().y() >= 0.0 && bounds.max().y() <= size[1])
            );
        }
    }
}

#[test]
fn death_immediate_respawn_finishes_the_backdrop_animation() {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let mut view = MenuView::new(true, "Player".into());
    view.screen = MenuScreen::Death;
    view.death_loading = true;
    view.death_presentation = launcher::menu::death::DeathPresentation::new(true, true);
    view.death_presentation.respawn_seconds = Some(0.0);
    view.death_presentation.advance(6.0);
    let metrics = TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2));
    let mut nodes = Vec::new();
    presentation
        .append_oreui_screen(
            &view,
            &mut nodes,
            &mut 1,
            metrics,
            [1280.0, 720.0],
            None,
            &|_| None,
        )
        .unwrap();
    let mesh = nodes
        .iter()
        .find_map(|node| match node.visual() {
            ui::UiVisual::Mesh(mesh) => Some(mesh),
            _ => None,
        })
        .expect("the backdrop remains visible while recovery is pending");
    assert_eq!(mesh.vertices()[0].uv, [-1.0, -1.0]);
    assert_eq!(mesh.vertices()[0].color[3], 102);
}

#[test]
fn home_owns_oreui_actions_and_directional_focus() {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    presentation.player_preview_icon = Some(ui::IconRef {
        page: 0,
        uv: [0, 0, 64, 64],
        glint: false,
    });
    let mut view = MenuView::new(true, "Player".into());
    view.auth_state = AuthState::Authenticated;
    let hits = append(&mut presentation, &view, [1280.0, 720.0]);
    for action in [
        MenuAction::Navigate(MenuScreen::Play),
        MenuAction::Navigate(MenuScreen::Servers),
        MenuAction::Navigate(MenuScreen::Settings),
        MenuAction::Navigate(MenuScreen::Social),
        MenuAction::Store(crate::store::OPEN),
        MenuAction::Navigate(MenuScreen::DressingRoom),
        MenuAction::Navigate(MenuScreen::Friends),
        MenuAction::Navigate(MenuScreen::Inbox),
        MenuAction::OpenAccounts,
        MenuAction::OpenExitDialog,
    ] {
        assert!(
            hits.iter().any(|(found, _)| *found == action),
            "missing {action:?}"
        );
        assert!(presentation.form_presentation.menu_focus.contains(&action));
    }
    assert!(presentation.menu_preview.control.is_some());
}

#[test]
fn create_world_tab_changes_keep_the_header_and_sidebar_mounted() {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let mut view = MenuView::new(true, "Player".into());
    view.screen = MenuScreen::Play;
    view.local.screen = launcher::local_worlds::Screen::Create;
    let metrics = TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2));
    let mut frame = |tab, seconds| {
        view.local.tab = tab;
        presentation.menu_seconds = seconds;
        let (mut nodes, mut next) = (Vec::new(), 1);
        presentation
            .append_oreui_screen(
                &view,
                &mut nodes,
                &mut next,
                metrics,
                [1280.0, 720.0],
                None,
                &|_| None,
            )
            .unwrap();
        presentation.end_animation_frame();
        nodes
    };
    frame(launcher::local_worlds::Tab::General, 0.0);
    let settled = frame(launcher::local_worlds::Tab::General, 0.2);
    let changed = frame(launcher::local_worlds::Tab::Advanced, 1.0);
    let header = |nodes: Vec<UiNode>| {
        nodes.into_iter().find(|node| matches!(node.visual(),
            ui::UiVisual::Text { layout, .. } if layout.glyphs().iter().map(|g| g.codepoint).collect::<String>().replace(' ', "") == "CreateNewWorld")).unwrap()
    };
    let (before, after) = (header(settled), header(changed));
    assert_eq!(before.bounds(), after.bounds());
    assert_eq!(before.visual(), after.visual());
}

#[test]
fn closing_add_server_animates_its_visuals_after_releasing_its_inputs() {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let mut view = MenuView::new(true, "Player".into());
    let metrics = TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2));
    let mut frame = |screen, seconds| {
        view.screen = screen;
        presentation.menu_seconds = seconds;
        let (mut nodes, mut next) = (Vec::new(), 1);
        let hits = presentation
            .append_oreui_screen(
                &view,
                &mut nodes,
                &mut next,
                metrics,
                [1280.0, 720.0],
                None,
                &|_| None,
            )
            .unwrap()
            .unwrap();
        presentation
            .append_oreui_motion(&mut nodes, &mut next, [1280.0, 720.0])
            .unwrap();
        presentation.end_animation_frame();
        (nodes, hits)
    };
    let title = |nodes: &[UiNode]| {
        nodes.iter().find_map(|node| match node.visual() {
            ui::UiVisual::Text { layout, color, .. }
                if layout
                    .glyphs()
                    .iter()
                    .map(|glyph| glyph.codepoint)
                    .collect::<String>()
                    .replace(' ', "")
                    == "ADDSERVER" =>
            {
                Some(color[3])
            }
            _ => None,
        })
    };
    frame(MenuScreen::AddServer, 0.0);
    let (_, hits) = frame(MenuScreen::Servers, 1.0);
    assert!(
        !hits
            .iter()
            .any(|(action, _)| *action == MenuAction::AddName)
    );
    let (closing, _) = frame(MenuScreen::Servers, 1.04);
    assert!(title(&closing).is_some_and(|alpha| alpha > 0 && alpha < 255));
    let (closed, _) = frame(MenuScreen::Servers, 1.09);
    assert_eq!(title(&closed), None);
}

#[test]
fn play_tabs_do_not_inherit_sounds_from_the_previous_screen() {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let mut view = MenuView::new(true, "Fixture".into());
    let tabs = [MenuScreen::Play, MenuScreen::Social, MenuScreen::Servers];
    presentation.form_presentation.menu_sounds.push((
        MenuAction::Navigate(MenuScreen::Profile),
        json_ui::ControlSound {
            name: "stale.screen.sound".into(),
            volume: 0.5,
            pitch: 0.5,
            min_seconds: 1.0,
        },
    ));
    for screen in [
        MenuScreen::Play,
        MenuScreen::Servers,
        MenuScreen::Social,
        MenuScreen::Play,
    ] {
        view.screen = screen;
        let hits = append(&mut presentation, &view, [1280.0, 720.0]);
        for tab in tabs {
            let action = MenuAction::Navigate(tab);
            let visible = hits.iter().any(|(candidate, _)| *candidate == action);
            assert_eq!(visible, tab != screen);
            assert!(presentation.menu_sound(action).is_none());
        }
        assert!(
            presentation
                .menu_sound(MenuAction::Navigate(MenuScreen::Profile))
                .is_none()
        );
    }
}

#[test]
fn add_server_dialog_owns_its_fields_and_blocks_background_controls() {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let mut view = MenuView::new(true, "Fixture".into());
    view.screen = MenuScreen::AddServer;
    view.name = "My server".into();
    view.address = "example.test".into();
    view.field = Some(MenuField::Address);
    let hits = append(&mut presentation, &view, [1280.0, 720.0]);
    let expected = [
        MenuAction::AddName,
        MenuAction::AddAddress,
        MenuAction::AddPort,
        MenuAction::AddBack,
        MenuAction::AddSave,
        MenuAction::AddSaveConnect,
    ];
    assert_eq!(hits.len(), expected.len());
    for action in expected {
        assert!(
            hits.iter().any(|(candidate, _)| *candidate == action),
            "missing {action:?}"
        );
    }
    for action in [
        MenuAction::AddName,
        MenuAction::AddAddress,
        MenuAction::AddPort,
    ] {
        let bounds = hits
            .iter()
            .find(|(candidate, _)| *candidate == action)
            .unwrap()
            .1;
        assert_eq!(
            presentation.menu_caret_at(bounds.min(), action.text_field().unwrap(), "value"),
            Some(0)
        );
    }
    assert_eq!(
        presentation.form_presentation.menu_focus.len(),
        expected.len()
    );
}

#[test]
fn add_server_requires_a_name_and_address_before_saving() {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let mut view = MenuView::new(true, "Fixture".into());
    view.screen = MenuScreen::AddServer;
    for (name, address) in [("", ""), ("My server", ""), ("", "example.test")] {
        view.name = name.into();
        view.address = address.into();
        let hits = append(&mut presentation, &view, [1280.0, 720.0]);
        assert!(
            !hits.iter().any(|(action, _)| matches!(
                action,
                MenuAction::AddSave | MenuAction::AddSaveConnect
            ))
        );
        assert!(
            hits.iter()
                .any(|(action, _)| *action == MenuAction::AddBack)
        );
    }
}

#[test]
fn add_server_footer_stays_visible_and_keyboard_reveals_the_port() {
    let mut view = MenuView::new(true, "Fixture".into());
    view.screen = MenuScreen::AddServer;
    view.name = "My server".into();
    view.address = "example.test".into();
    view.message =
        Some("The server could not be saved. Check the address and try again. ".repeat(5));
    for size in [[1280.0, 720.0], [640.0, 320.0], [320.0, 560.0]] {
        let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
        view.field = Some(MenuField::Port);
        view.focused_action = Some(MenuAction::AddPort);
        view.navigation_focus_visible = true;
        let hits = append(&mut presentation, &view, size);
        for action in [
            MenuAction::AddPort,
            MenuAction::AddBack,
            MenuAction::AddSave,
            MenuAction::AddSaveConnect,
        ] {
            let bounds = hits
                .iter()
                .find(|(candidate, _)| *candidate == action)
                .unwrap_or_else(|| panic!("{action:?} must be reachable at {size:?}"))
                .1;
            assert!(bounds.min().x() >= 0.0 && bounds.min().y() >= 0.0);
            assert!(bounds.max().x() <= size[0] && bounds.max().y() <= size[1]);
        }
        let offset = presentation
            .menu_scrolls
            .offsets()
            .get("add_server_fields")
            .copied()
            .unwrap_or(0.0);
        view.focused_action = Some(MenuAction::AddSaveConnect);
        append(&mut presentation, &view, size);
        assert_eq!(
            presentation
                .menu_scrolls
                .offsets()
                .get("add_server_fields")
                .copied()
                .unwrap_or(0.0),
            offset,
            "footer focus must not scroll the draft"
        );
    }
}

#[test]
fn pause_offers_the_wardrobe_and_keeps_every_action_on_screen() {
    for size in [[1280.0, 720.0], [640.0, 320.0], [320.0, 560.0]] {
        let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
        let mut view = MenuView::new(true, "Fixture".into());
        view.screen = MenuScreen::Pause;
        view.over_world = true;
        let hits = append(&mut presentation, &view, size);
        for action in [
            MenuAction::PauseResume,
            MenuAction::PauseSettings,
            MenuAction::Navigate(MenuScreen::DressingRoom),
            MenuAction::PauseDisconnect,
        ] {
            let bounds = hits
                .iter()
                .find(|(candidate, _)| *candidate == action)
                .unwrap()
                .1;
            assert!(bounds.min().x() >= 0.0 && bounds.min().y() >= 0.0);
            assert!(bounds.max().x() <= size[0] && bounds.max().y() <= size[1]);
        }
    }
}

#[test]
fn connection_screen_owns_cancel_and_blocks_the_play_tabs() {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let mut view = MenuView::new(true, "Fixture".into());
    view.screen = MenuScreen::Servers;
    view.connecting = true;
    let hits = append(&mut presentation, &view, [1280.0, 720.0]);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].0, MenuAction::AddBack);
    view.feeds.join = launcher::menu::JoinProgress::new(launcher::menu::JoinKind::Realm);
    view.feeds
        .join
        .observe(Some(launcher::menu::JoinStage::Realm));
    assert!(append(&mut presentation, &view, [1280.0, 720.0]).is_empty());
}

#[test]
fn pause_dims_the_whole_world_evenly_and_keeps_the_character_clear_of_its_actions() {
    let mut view = MenuView::new(true, "Fixture".into());
    view.screen = MenuScreen::Pause;
    view.over_world = true;
    let mut preview = None;
    let (_, hits, nodes) = super::review_tests::paint(Default::default(), |canvas| {
        preview = super::pause::draw(canvas, &view, [1280.0, 720.0], &|_| None).unwrap();
    });
    let overlays: Vec<_> = super::review_tests::solids(&nodes)
        .into_iter()
        .filter(|(_, color)| *color == super::theme::OVERLAY_SCREEN)
        .collect();
    assert_eq!(
        overlays,
        vec![([0.0, 0.0, 1280.0, 720.0], super::theme::OVERLAY_SCREEN)]
    );
    let preview = preview.expect("desktop Pause includes the interactive character");
    let wardrobe = hits
        .iter()
        .find(|(action, _)| *action == MenuAction::Navigate(MenuScreen::DressingRoom))
        .unwrap()
        .1;
    assert!(preview.control[1] < preview.control[3]);
    assert!(preview.control[3] < wardrobe.min().y());
    assert!(preview.control[0] >= wardrobe.min().x());
    assert!(preview.control[2] <= wardrobe.max().x());
}

#[test]
fn pause_card_has_no_green_stripe_above_the_logo() {
    let view = MenuView::new(true, "Fixture".into());
    let (_, hits, nodes) = super::review_tests::paint(Default::default(), |canvas| {
        super::pause::draw(canvas, &view, [1280.0, 720.0], &|_| None).unwrap();
    });
    let resume = hits
        .iter()
        .find(|(action, _)| *action == MenuAction::PauseResume)
        .unwrap()
        .1;
    for (bounds, color) in super::review_tests::solids(&nodes) {
        if color == super::theme::PRIMARY_ROLE.fill {
            assert!(
                bounds[1] >= resume.min().y() && bounds[3] <= resume.max().y(),
                "green belongs to the Resume action, not the card edge"
            );
        }
    }
}

#[test]
fn local_world_progress_cancel_keeps_its_existing_backend_action() {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let mut view = MenuView::new(true, "Fixture".into());
    view.local.progress = Some(launcher::local_worlds::Progress {
        stage: launcher::local_worlds::Stage::DownloadingServer,
        fraction: Some(0.5),
        detail: "50 / 100 MB".into(),
    });
    let hits = append(&mut presentation, &view, [1280.0, 720.0]);
    assert_eq!(hits.len(), 1);
    assert_eq!(
        hits[0].0,
        MenuAction::LocalWorld(launcher::menu::LocalWorldAction::Back)
    );
    view.local.progress = Some(launcher::local_worlds::Progress::connecting(
        "Fixture world",
    ));
    assert!(append(&mut presentation, &view, [1280.0, 720.0]).is_empty());
}

#[test]
fn add_server_dismissal_is_a_corner_control_above_the_fields() {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let mut view = MenuView::new(true, "Fixture".into());
    view.screen = MenuScreen::AddServer;
    for size in [[1280.0, 720.0], [640.0, 320.0], [320.0, 560.0]] {
        let hits = append(&mut presentation, &view, size);
        let bounds = |action| hits.iter().find(|(found, _)| *found == action).unwrap().1;
        let close = bounds(MenuAction::AddBack);
        let name = bounds(MenuAction::AddName);
        assert!(close.max().y() < name.min().y());
        assert!((close.width() - close.height()).abs() < 0.01);
        assert!(close.min().x() > name.min().x());
    }
}

#[test]
fn pause_resume_follows_its_caption_without_a_large_empty_gap() {
    let view = MenuView::new(true, "Fixture".into());
    let mut rem = 0.0;
    let (_, hits, nodes) = super::review_tests::paint(Default::default(), |canvas| {
        rem = canvas.rem;
        super::pause::draw(canvas, &view, [1280.0, 720.0], &|_| None).unwrap();
    });
    let caption = nodes.iter().find(|node| matches!(node.visual(), ui::UiVisual::Text { layout, .. }
        if layout.glyphs().iter().map(|glyph| glyph.codepoint).collect::<String>() == "Pick up where you left off."
    )).unwrap().bounds();
    let resume = hits
        .iter()
        .find(|(action, _)| *action == MenuAction::PauseResume)
        .unwrap()
        .1;
    let gap = resume.min().y() - caption.max().y();
    assert!(
        gap >= 0.0 && gap <= rem * 2.4,
        "caption-to-action gap: {gap}"
    );
}

#[test]
fn pause_actions_align_with_the_character_footer_and_logo_has_extra_top_space() {
    let view = MenuView::new(true, "Fixture".into());
    let (_, hits, nodes) = super::review_tests::paint(Default::default(), |canvas| {
        canvas.title_artwork = Some(ui::IconRef {
            page: 1,
            uv: [0, 0, 300, 100],
            glint: false,
        });
        super::pause::draw(canvas, &view, [1280.0, 720.0], &|_| None).unwrap();
    });
    let bounds = |action| hits.iter().find(|(found, _)| *found == action).unwrap().1;
    let leave = bounds(MenuAction::PauseDisconnect);
    let wardrobe = bounds(MenuAction::Navigate(MenuScreen::DressingRoom));
    assert!(
        (leave.max().y() - wardrobe.max().y()).abs() < 0.01,
        "the menu and character actions share the panel's bottom inset"
    );
    let heading = nodes.iter().find(|node| matches!(node.visual(), ui::UiVisual::Text { layout, .. }
        if layout.glyphs().iter().map(|glyph| glyph.codepoint).collect::<String>() == "YOUR CHARACTER"
    )).unwrap().bounds();
    let logo = nodes
        .iter()
        .find(|node| {
            matches!(
                node.visual(),
                ui::UiVisual::Sprite {
                    texture_page: 1,
                    ..
                }
            )
        })
        .unwrap()
        .bounds();
    assert!(
        logo.min().y() > heading.min().y(),
        "the logo has more top breathing room than the text heading"
    );
}

#[test]
fn focused_fields_respect_the_blink_phase_and_hide_carets_during_selection() {
    let mut view = MenuView::new(true, "Fixture".into());
    view.field = Some(MenuField::Name);
    for (shown, selection, expected) in [
        (true, None, true),
        (false, None, false),
        (true, Some([0, 2]), false),
    ] {
        view.caret.shown = shown;
        view.caret.selection = selection;
        let (_, _, nodes) = super::review_tests::paint(Default::default(), |canvas| {
            super::widgets::text_field(
                canvas,
                &view,
                [10.0, 20.0, 410.0, 68.0],
                "Text",
                "",
                true,
                Some(MenuAction::AddName),
            )
            .unwrap();
        });
        let caret = super::review_tests::solids(&nodes)
            .iter()
            .any(|(_, color)| *color == super::theme::FIELD_CARET);
        assert_eq!(caret, expected);
    }
}

#[test]
fn play_tabs_draw_the_installed_native_icons() {
    use crate::ui_runtime::oreui_assets::{PLAY_TAB_ICONS, load_optional_oreui_images};
    let Some(images) = load_optional_oreui_images() else {
        eprintln!(
            "skipping play_tabs_draw_the_installed_native_icons: installed OreUI bundle unavailable"
        );
        return;
    };
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    presentation.enable_oreui_originals(images).unwrap();
    let mut view = MenuView::new(true, "Fixture".into());
    view.screen = MenuScreen::Play;
    let metrics = TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2));
    let (mut nodes, mut next) = (Vec::new(), 1);
    presentation
        .append_oreui_screen(
            &view,
            &mut nodes,
            &mut next,
            metrics,
            [1280.0, 720.0],
            None,
            &|_| None,
        )
        .unwrap();
    let originals = presentation
        .form_presentation
        .oreui_originals
        .as_ref()
        .unwrap();
    for key in PLAY_TAB_ICONS {
        let sprite = originals.sprites[key];
        assert!(
            nodes.iter().any(
                |node| matches!(node.visual(), ui::UiVisual::Sprite { texture_page, uv, .. }
            if *texture_page == originals.page + sprite.page && *uv == sprite.bounds)
            ),
            "missing Play tab icon {key}"
        );
    }
}

#[test]
fn text_fields_keep_their_inset_even_on_every_edge() {
    let mut view = MenuView::new(true, "Fixture".into());
    view.hovered = Some(MenuAction::AddName);
    let bounds = [10.0, 20.0, 410.0, 68.0];
    for focused in [false, true] {
        let (_, _, nodes) = super::review_tests::paint(Default::default(), |canvas| {
            super::widgets::text_field(
                canvas,
                &view,
                bounds,
                "",
                "My server",
                focused,
                Some(MenuAction::AddName),
            )
            .unwrap();
        });
        let (face, _) = super::review_tests::solids(&nodes)
            .into_iter()
            .find(|(_, color)| *color == super::theme::NEUTRAL80.hovered)
            .unwrap();
        let insets = [
            face[0] - bounds[0],
            face[1] - bounds[1],
            bounds[2] - face[2],
            bounds[3] - face[3],
        ];
        assert!(
            insets.iter().all(|value| (*value - insets[0]).abs() < 0.01),
            "field insets: {insets:?}"
        );
    }
}

#[test]
fn text_fields_center_visible_glyphs_instead_of_the_font_line_box() {
    use crate::ui_runtime::oreui_fonts::OreUiFont;
    let base = fixture_font();
    let native = base
        .as_ref()
        .clone()
        .with_line_metrics(assets::FontLineMetrics {
            em_64: 32 * 64,
            ascent_64: 26 * 64,
            descent_64: 3 * 64,
        })
        .unwrap();
    let font = base
        .with_named_font(OreUiFont::Seven.name(), &native)
        .unwrap();
    let metrics = TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2));
    let view = MenuView::new(true, "Fixture".into());
    for hovered in [None, Some(MenuAction::AddName)] {
        let mut view = view.clone();
        view.hovered = hovered;
        let (mut nodes, mut next, mut layouts) =
            (Vec::new(), 1, ui::TextLayoutCache::new(16, 65536));
        let mut canvas = Canvas::new(&mut nodes, &mut next, &mut layouts, &font, metrics, 0, None);
        super::widgets::text_field(
            &mut canvas,
            &view,
            [0.0, 0.0, 400.0, 48.0],
            "",
            "My server",
            false,
            Some(MenuAction::AddName),
        )
        .unwrap();
        let ink = nodes
            .iter()
            .filter_map(|node| {
                let ui::UiVisual::Text { layout, .. } = node.visual() else {
                    return None;
                };
                Some(
                    layout
                        .glyphs()
                        .iter()
                        .filter(|glyph| !glyph.codepoint.is_whitespace())
                        .fold(
                            [f32::INFINITY, f32::NEG_INFINITY],
                            |[top, bottom], glyph| {
                                [
                                    top.min(
                                        node.bounds().min().y() + glyph.bounds_64[1] as f32 / 64.0,
                                    ),
                                    bottom.max(
                                        node.bounds().min().y() + glyph.bounds_64[3] as f32 / 64.0,
                                    ),
                                ]
                            },
                        ),
                )
            })
            .next()
            .unwrap();
        assert!(
            ((ink[0] + ink[1]) * 0.5 - 24.0).abs() <= 0.5,
            "visible glyph bounds: {ink:?}"
        );
    }
}

#[test]
fn pause_loads_small_server_logos_without_an_oversized_artwork_copy() {
    use super::super::super::menu_artwork::TITLE_KEY;
    let mut presentation = crate::test_support::mini_engine_presentation();
    let mut bytes = Vec::new();
    image::RgbaImage::from_pixel(8, 4, image::Rgba([73, 91, 37, 255]))
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
    presentation.set_server_ui_pack(&super::super::ServerUiPack {
        textures: vec![(format!("{TITLE_KEY}.png"), bytes)],
        ..Default::default()
    });
    let mut view = MenuView::new(true, "Fixture".into());
    view.screen = MenuScreen::Pause;
    view.over_world = true;
    let mut nodes = Vec::new();
    let size = [1280.0, 720.0];
    let metrics = TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2));
    presentation
        .append_oreui_screen(&view, &mut nodes, &mut 1, metrics, size, None, &|_| None)
        .unwrap();
    let engine = presentation.form_presentation.engine.as_deref().unwrap();
    let atlas = engine.textures.lock();
    let resident = atlas
        .placement(TITLE_KEY)
        .expect("a Pause logo is requested even without JSON title controls");
    let page = engine.textures.server_page + resident.page;
    assert!(nodes.iter().any(|node| matches!(node.visual(), ui::UiVisual::Sprite { texture_page, .. } if *texture_page == page)));
}

#[test]
fn pause_character_name_follows_the_current_account_and_profile() {
    let mut view = MenuView::new(true, launcher::PRODUCT_NAME.into());
    view.feeds.accounts = vec![launcher::accounts::AccountProfile {
        id: "active".into(),
        gamertag: "Saved username".into(),
        ..Default::default()
    }];
    view.feeds.account_active_id = Some("active".into());
    for (profile_name, wanted) in [
        ("", "Saved username"),
        ("Updated username", "Updated username"),
    ] {
        view.feeds.profile.gamertag = profile_name.into();
        let (_, _, nodes) = super::review_tests::paint(Default::default(), |canvas| {
            super::pause::draw(canvas, &view, [1280.0, 720.0], &|_| None).unwrap();
        });
        let labels = nodes
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
            .collect::<Vec<_>>();
        assert!(labels.iter().any(|label| label == wanted), "{labels:?}");
        assert!(!labels.iter().any(|label| label == launcher::PRODUCT_NAME));
    }
}

#[test]
fn death_hardcore_offers_exit_and_spectating() {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let mut view = MenuView::new(true, "Player".into());
    view.screen = MenuScreen::Death;
    view.death_presentation.hardcore = true;
    let hits = append(&mut presentation, &view, [1280.0, 720.0]);
    assert_eq!(
        hits.iter().map(|(action, _)| *action).collect::<Vec<_>>(),
        [MenuAction::DeathExitWorld, MenuAction::Respawn]
    );
}

#[test]
fn death_actions_follow_stages_and_retire_during_respawn() {
    for animations in [false, true] {
        let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
        let mut view = MenuView::new(true, "Player".into());
        view.screen = MenuScreen::Death;
        view.death_presentation = launcher::menu::death::DeathPresentation::new(animations, false);
        view.death_presentation
            .advance(view.death_presentation.controls_at() - 0.01);
        assert!(append(&mut presentation, &view, [1280.0, 720.0]).is_empty());
        view.death_presentation.advance(0.01);
        assert_eq!(append(&mut presentation, &view, [1280.0, 720.0]).len(), 2);
        view.death_loading = true;
        view.death_presentation.respawn_seconds = Some(0.0);
        assert!(append(&mut presentation, &view, [1280.0, 720.0]).is_empty());
    }
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let mut view = MenuView::new(true, "Player".into());
    view.screen = MenuScreen::Death;
    view.death_presentation = launcher::menu::death::DeathPresentation::new(true, true);
    view.death_presentation.advance(10.0);
    assert!(append(&mut presentation, &view, [1280.0, 720.0]).is_empty());
}

#[test]
fn signed_in_realms_offer_membership_without_an_existing_realm() {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let mut view = MenuView::new(true, "Player".into());
    view.screen = MenuScreen::Social;
    view.auth_state = AuthState::Authenticated;
    let action = MenuAction::RealmMembership(launcher::menu::realm_membership::Action::Open);
    assert!(
        append(&mut presentation, &view, [1280.0, 720.0])
            .iter()
            .any(|(hit, _)| *hit == action)
    );
    view.auth_state = AuthState::SignedOut;
    assert!(
        !append(&mut presentation, &view, [1280.0, 720.0])
            .iter()
            .any(|(hit, _)| *hit == action)
    );
}

#[test]
fn realm_membership_busy_flow_excludes_underlying_tabs_and_duplicate_requests() {
    use launcher::menu::realm_membership::{Action, Stage, State};
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let mut view = MenuView::new(true, "Fixture".into());
    view.screen = MenuScreen::Social;
    view.auth_state = AuthState::Authenticated;
    for stage in [Stage::Code, Stage::Verifying, Stage::Joining] {
        view.realm_membership = Some(State {
            stage,
            ..Default::default()
        });
        for size in [[1280.0, 720.0], [640.0, 360.0]] {
            let hits = append(&mut presentation, &view, size);
            assert!(
                hits.iter()
                    .all(|(action, _)| matches!(action, MenuAction::RealmMembership(_)))
            );
            assert!(!hits.iter().any(|(action, _)| matches!(
                action,
                MenuAction::RealmMembership(Action::Verify | Action::Accept)
            )));
            assert_eq!(hits.is_empty(), stage == Stage::Joining);
            assert!(
                hits.iter()
                    .all(|(_, rect)| rect.min().y() >= 0.0 && rect.max().y() <= size[1])
            );
        }
    }
}

#[test]
fn realm_membership_completion_disables_unavailable_play() {
    use launcher::menu::realm_membership::{Action, Stage, State};
    for (realm_state, expired, available) in [
        ("OPEN", false, true),
        ("CLOSED", false, false),
        ("OPEN", true, false),
    ] {
        let mut view = MenuView::new(true, "Fixture".into());
        let realm = serde_json::from_value(serde_json::json!({"name":"Fixture Realm", "state":realm_state, "expired":expired, "target":"realm_id/7"})).unwrap();
        view.realm_membership = Some(State {
            stage: Stage::Complete,
            realm: Some(realm),
            ..Default::default()
        });
        let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
        let hits = append(&mut presentation, &view, [1280.0, 720.0]);
        assert_eq!(
            hits.iter()
                .any(|(action, _)| *action == MenuAction::RealmMembership(Action::Play)),
            available
        );
        assert!(
            hits.iter()
                .any(|(action, _)| *action == MenuAction::RealmMembership(Action::Back))
        );
    }
}
