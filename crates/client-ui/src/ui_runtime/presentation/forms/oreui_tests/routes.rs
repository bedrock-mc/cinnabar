//! OreUI routes through the menu adapter: their actions, focus, artwork and sounds.

use launcher::menu::settings_options::SettingId;
use std::sync::Arc;

use super::super::super::UiPresentationRuntime;
use launcher::menu::auth::AuthState;
use launcher::menu::settings_options::SETTINGS_OPTIONS;
use launcher::menu::{MenuAction, MenuField, MenuScreen, MenuView};
use ui::{UiNode, UiRect};
use ui_engine::test_support::fixture_font;
use ui_engine::text_metrics::TextMetrics;

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
        color_mask: None,
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
        assert!(presentation.projections.menu_focus.contains(&action));
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
    presentation.screens.menu_sounds.push((
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
    assert_eq!(presentation.projections.menu_focus.len(), expected.len());
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

/// The same focused field is revealed again when the keyboard changes the usable height.
#[test]
fn add_server_port_remains_visible_when_keyboard_shrinks_the_existing_viewport() {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let mut view = MenuView::new(true, "Fixture".into());
    view.screen = MenuScreen::AddServer;
    view.field = Some(MenuField::Port);
    view.focused_action = Some(MenuAction::AddPort);
    for size in [
        [1280.0, 720.0],
        [1280.0, 280.0],
        [1280.0, 720.0],
        [1280.0, 280.0],
    ] {
        let hits = append(&mut presentation, &view, size);
        let port = hits
            .iter()
            .find(|(action, _)| *action == MenuAction::AddPort)
            .unwrap()
            .1;
        assert!(
            port.min().y() >= 0.0 && port.max().y() <= size[1],
            "port clipped at {size:?}: {port:?}"
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
fn play_tabs_draw_the_installed_native_icons() {
    use oreui::images::{PLAY_TAB_ICONS, checkout_compiled_dir, load_optional_oreui_images};
    let Some(images) = load_optional_oreui_images(&checkout_compiled_dir()) else {
        eprintln!(
            "skipping play_tabs_draw_the_installed_native_icons: compiled OreUI artwork carrier unavailable"
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
    let originals = presentation.screens.oreui.originals.as_ref().unwrap();
    for key in PLAY_TAB_ICONS {
        let sprite = originals.sprite(key).unwrap();
        assert!(
            nodes.iter().any(
                |node| matches!(node.visual(), ui::UiVisual::Sprite { texture_page, uv, .. }
            if *texture_page == originals.page() + sprite.page && *uv == sprite.bounds)
            ),
            "missing Play tab icon {key}"
        );
    }
}

#[test]
fn pause_loads_small_server_logos_without_an_oversized_artwork_copy() {
    use ui_engine::textures::TITLE_KEY;
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
    let engine = presentation.engine.as_deref().unwrap();
    let atlas = engine.textures.lock();
    let resident = atlas
        .placement(TITLE_KEY)
        .expect("a Pause logo is requested even without JSON title controls");
    let page = engine.textures.server_page + resident.page;
    assert!(nodes.iter().any(|node| matches!(node.visual(), ui::UiVisual::Sprite { texture_page, .. } if *texture_page == page)));
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

#[test]
fn pause_quit_uses_installed_translation_through_the_screen_route() {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let mut view = MenuView::new(true, "Player".into());
    view.screen = MenuScreen::Pause;
    let metrics = TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2));
    let (mut nodes, mut next) = (Vec::new(), 1);
    let hits = presentation
        .append_oreui_screen(
            &view,
            &mut nodes,
            &mut next,
            metrics,
            [1280.0, 720.0],
            None,
            &|key| (key == "pauseScreen.quit").then(|| "Quitter".into()),
        )
        .unwrap()
        .unwrap();
    assert!(
        hits.iter()
            .any(|(action, _)| *action == MenuAction::PauseDisconnect)
    );
    assert!(
        presentation
            .projections
            .menu_focus
            .contains(&MenuAction::PauseDisconnect)
    );
    assert!(nodes.iter().any(
        |node| matches!(node.visual(), ui::UiVisual::Text { layout, .. }
        if layout.glyphs().iter().map(|glyph| glyph.codepoint).collect::<String>() == "Quitter")
    ));
}

/// The persisted animation setting reaches OreUI's timelines through the motion adapter: a
/// later route's entrance plays only while the setting allows motion.
#[test]
fn screen_animation_setting_drives_oreui_motion_through_the_adapter() {
    let index = SETTINGS_OPTIONS
        .iter()
        .position(|option| option.id == SettingId::ScreenAnimations)
        .unwrap();
    for (setting, animates) in [(0, false), (1, true)] {
        let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
        let mut view = MenuView::new(true, "Player".into());
        Arc::make_mut(&mut view.settings_options).set(index, setting);
        // A first frame primes the timelines; motion is on until the adapter reads the setting.
        append(&mut presentation, &view, [1280.0, 720.0]);
        presentation.end_animation_frame();
        // Friends starts no icon highlight, so only its entrance can keep motion active.
        view.screen = MenuScreen::Friends;
        presentation.set_menu_view(Some(view.clone()));
        presentation.configure_oreui_settings();
        presentation.menu_seconds = 1.0;
        append(&mut presentation, &view, [1280.0, 720.0]);
        // Midway through the route's entrance.
        assert_eq!(
            presentation.screens.oreui.transitions.active(1.05),
            animates,
            "screen_animations = {setting}"
        );
    }
}
