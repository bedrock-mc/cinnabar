//! Accessibility sizing through the same screen entry point the client paints.

#[path = "accessibility_overlay_tests.rs"]
mod overlays;
#[path = "accessibility_route_tests.rs"]
mod routes;

use std::{collections::HashMap, sync::Arc};

use launcher::menu::{
    MenuAction, MenuScreen, MenuView,
    settings_options::{EXTRA_LARGE_UI, SettingsOptions},
};
use ui::{DpiScale, SafeArea, TextLayoutCache, UiNode, UiPoint, UiRect};
use ui_engine::{
    engine::screen::{PaintFrame, Screen},
    menu_scroll::MenuScrolls,
    test_support::fixture_font,
    text_metrics::TextMetrics,
};

use crate::{
    OreUiScreen,
    menu_screen::{MenuGeometry, MenuInput},
    test_support::option_index,
};

struct Painted {
    nodes: Vec<UiNode>,
    hits: Vec<(MenuAction, UiRect)>,
}

#[test]
fn extra_large_friends_keeps_close_and_scrolled_friends_reachable_in_small_windows() {
    for size in [ui::MIN_WINDOW_LOGICAL_SIZE, [400.0, 250.0], [752.0, 500.0]] {
        for dpi in [1.0, 2.0] {
            for enabled in [0, 1] {
                let mut view = MenuView::new(true, "Fixture".into());
                view.screen = MenuScreen::Friends;
                view.friends = (0..32)
                    .map(|index| launcher::menu::MenuFriendCard {
                        gamertag: format!("Fixture {index}"),
                        world_name: "World".into(),
                        members: String::new(),
                        xuid: format!("fixture-{index}"),
                        max_members: 0,
                    })
                    .collect();
                Arc::make_mut(&mut view.settings_options)
                    .set(option_index(EXTRA_LARGE_UI), enabled);
                let metrics = TextMetrics::for_viewport(
                    size.map(|value| (value * dpi) as u32),
                    DpiScale::new(dpi).unwrap(),
                    Some(2),
                );
                let mut screen = OreUiScreen::default();
                let mut scrolls = MenuScrolls::default();
                let frame = paint_with_scrolls(&mut screen, &view, metrics, size, &mut scrolls);
                for action in [
                    MenuAction::Navigate(MenuScreen::Home),
                    MenuAction::PlayFriend(0),
                ] {
                    let bounds = frame
                        .hits
                        .iter()
                        .find_map(|(at, bounds)| (*at == action).then_some(*bounds))
                        .expect("Friends keeps its Close and first friend targets reachable");
                    assert!(bounds.width() > 0.0 && bounds.height() > 0.0);
                    assert!(bounds.min().x() >= 0.0 && bounds.min().y() >= 0.0);
                    assert!(bounds.max().x() <= size[0] && bounds.max().y() <= size[1]);
                }
                let areas = screen.take_paint().unwrap().scrolls;
                let list = areas
                    .iter()
                    .find(|area| area.key == "friends_list")
                    .unwrap();
                assert!(list.viewport.width() > 0.0 && list.viewport.height() > 0.0);
                assert!(list.max > 0.0);
                let point = crate::test_support::centre(list.viewport);
                let delta = -list.max;
                scrolls.set_areas(areas);
                assert!(scrolls.wheel(point, delta, true));
                let frame = paint_with_scrolls(&mut screen, &view, metrics, size, &mut scrolls);
                assert!(
                    frame.hits.iter().any(|(action, bounds)| {
                        *action == MenuAction::PlayFriend(view.friends.len() - 1)
                            && bounds.width() > 0.0
                            && bounds.height() > 0.0
                    }),
                    "the last friend is reachable through real scrolling"
                );
            }
        }
    }
}

#[test]
fn extra_large_realms_keeps_selected_play_reachable_by_scrolling() {
    for size in [
        [1280.0, 500.0],
        ui::MIN_WINDOW_LOGICAL_SIZE,
        [400.0, 250.0],
        [752.0, 500.0],
    ] {
        for dpi in [1.0, 2.0] {
            for enabled in [0, 1] {
                let mut view = MenuView::new(true, "Fixture".into());
                view.screen = MenuScreen::Social;
                view.auth_state = launcher::menu::auth::AuthState::Authenticated;
                view.realms = vec![launcher::menu::MenuRealmCard {
                    name: "Realm".into(),
                    state: "open".into(),
                    target: String::new(),
                    address: String::new(),
                    owner: "Fixture".into(),
                    online_players: 0,
                    max_players: 0,
                    days_left: 0,
                    expired: false,
                    member: false,
                }]
                .into();
                view.feeds.selected_realm = Some(0);
                Arc::make_mut(&mut view.settings_options)
                    .set(option_index(EXTRA_LARGE_UI), enabled);
                let metrics = TextMetrics::for_viewport(
                    size.map(|value| (value * dpi) as u32),
                    DpiScale::new(dpi).unwrap(),
                    Some(2),
                );
                let mut screen = OreUiScreen::default();
                let mut scrolls = MenuScrolls::default();
                let mut reached = false;
                for _ in 0..32 {
                    let frame = paint_with_scrolls(&mut screen, &view, metrics, size, &mut scrolls);
                    reached = frame.hits.iter().any(|(action, bounds)| {
                        *action == MenuAction::PlayRealm(0)
                            && bounds.width() > 0.0
                            && bounds.height() > 0.0
                            && bounds.min().x() >= 0.0
                            && bounds.min().y() >= 0.0
                            && bounds.max().x() <= size[0]
                            && bounds.max().y() <= size[1]
                    });
                    if reached {
                        break;
                    }
                    let areas = screen.take_paint().unwrap().scrolls;
                    let details = areas
                        .iter()
                        .find(|area| area.key == "realm_details")
                        .expect("overflowing Realm details must let the pointer reach Play");
                    assert!(details.viewport.width() > 0.0 && details.viewport.height() > 0.0);
                    let point = crate::test_support::centre(details.viewport);
                    let delta = -details.max;
                    scrolls.set_areas(areas);
                    assert!(scrolls.wheel(point, delta, true));
                }
                assert!(
                    reached,
                    "selected Realm's Play target is reachable: {size:?}, DPI {dpi}, enabled {enabled}"
                );
            }
        }
    }
}

#[test]
fn extra_large_play_keeps_loaded_worlds_and_navigation_reachable_in_small_windows() {
    for enabled in [0, 1] {
        for size in [ui::MIN_WINDOW_LOGICAL_SIZE, [400.0, 250.0], [752.0, 500.0]] {
            for dpi in [1.0, 2.0] {
                let mut view = MenuView::new(true, "Fixture".into());
                view.screen = MenuScreen::Play;
                view.local_worlds = (0..32)
                    .map(|index| launcher::menu::LocalWorldCard {
                        name: format!("Fixture {index}"),
                        ..Default::default()
                    })
                    .collect();
                Arc::make_mut(&mut view.settings_options)
                    .set(option_index(EXTRA_LARGE_UI), enabled);
                let metrics = TextMetrics::for_viewport(
                    size.map(|value| (value * dpi) as u32),
                    DpiScale::new(dpi).unwrap(),
                    Some(2),
                );
                let mut screen = OreUiScreen::default();
                let mut scrolls = MenuScrolls::default();
                let frame = paint_with_scrolls(&mut screen, &view, metrics, size, &mut scrolls);
                for action in [
                    MenuAction::Navigate(MenuScreen::Home),
                    MenuAction::Navigate(MenuScreen::Servers),
                    MenuAction::LocalWorld(launcher::menu::LocalWorldAction::BeginCreate),
                    MenuAction::PlayLocalWorld(0),
                ] {
                    let bounds = frame
                        .hits
                        .iter()
                        .find_map(|(at, bounds)| (*at == action).then_some(*bounds))
                        .expect("Play keeps navigation, creation and its first world reachable");
                    assert!(bounds.width() > 0.0 && bounds.height() > 0.0);
                    assert!(bounds.min().x() >= 0.0 && bounds.min().y() >= 0.0);
                    assert!(bounds.max().x() <= size[0] && bounds.max().y() <= size[1]);
                }
                let areas = screen.take_paint().unwrap().scrolls;
                let worlds = areas.iter().find(|area| area.key == "worlds_list").unwrap();
                assert!(worlds.viewport.width() > 0.0 && worlds.viewport.height() > 0.0);
                assert!(worlds.max > 0.0, "many worlds remain scrollable");
                let point = crate::test_support::centre(worlds.viewport);
                let delta = -worlds.max;
                scrolls.set_areas(areas);
                assert!(scrolls.wheel(point, delta, true));
                let frame = paint_with_scrolls(&mut screen, &view, metrics, size, &mut scrolls);
                assert!(
                    frame.hits.iter().any(|(action, bounds)| {
                        *action == MenuAction::PlayLocalWorld(view.local_worlds.len() - 1)
                            && bounds.width() > 0.0
                            && bounds.height() > 0.0
                    }),
                    "the last world is reachable through actual scrolling"
                );
            }
        }
    }
}

#[test]
fn extra_large_loaded_profile_keeps_tabs_and_scrolled_actions_reachable_in_small_windows() {
    use launcher::menu::{ProfileTab, auth::AuthState};
    for enabled in [0, 1] {
        for size in [ui::MIN_WINDOW_LOGICAL_SIZE, [400.0, 250.0], [752.0, 500.0]] {
            for dpi in [1.0, 2.0] {
                for tab in [ProfileTab::Overview, ProfileTab::Stats] {
                    let mut view = MenuView::new(true, "Fixture".into());
                    view.screen = MenuScreen::Profile;
                    view.auth_state = AuthState::Authenticated;
                    view.profile_tab = tab;
                    view.feeds.profile.loaded = true;
                    view.feeds.profile.avatar_loaded = true;
                    view.feeds.profile.featured_screenshot_loaded = true;
                    view.feeds.profile.friends = Some(1);
                    Arc::make_mut(&mut view.settings_options)
                        .set(option_index(EXTRA_LARGE_UI), enabled);
                    let metrics = TextMetrics::for_viewport(
                        size.map(|value| (value * dpi) as u32),
                        DpiScale::new(dpi).unwrap(),
                        Some(2),
                    );
                    let mut screen = OreUiScreen::default();
                    let mut scrolls = MenuScrolls::default();
                    let mut dressing_room_reached = false;
                    for _ in 0..32 {
                        let frame =
                            paint_with_scrolls(&mut screen, &view, metrics, size, &mut scrolls);
                        let (selected, other) = match tab {
                            ProfileTab::Overview => ("Overview", ProfileTab::Stats),
                            ProfileTab::Stats => ("Stats", ProfileTab::Overview),
                        };
                        for action in [MenuAction::AddBack, MenuAction::SelectProfileTab(other)] {
                            let bounds = frame
                                .hits
                                .iter()
                                .find_map(|(at, bounds)| (*at == action).then_some(*bounds))
                                .expect("Profile keeps its fixed navigation reachable");
                            assert!(bounds.min().x() >= 0.0 && bounds.min().y() >= 0.0);
                            assert!(bounds.max().x() <= size[0] && bounds.max().y() <= size[1]);
                        }
                        let selected_bounds = frame
                            .nodes
                            .iter()
                            .find_map(|node| {
                                if let ui::UiVisual::Text { layout, .. } = node.visual()
                                    && layout
                                        .glyphs()
                                        .iter()
                                        .map(|glyph| glyph.codepoint)
                                        .collect::<String>()
                                        == selected
                                {
                                    Some(node.bounds())
                                } else {
                                    None
                                }
                            })
                            .expect("the active tab remains visibly labeled");
                        assert!(
                            selected_bounds.min().x() >= 0.0 && selected_bounds.min().y() >= 0.0
                        );
                        assert!(
                            selected_bounds.max().x() <= size[0]
                                && selected_bounds.max().y() <= size[1]
                        );
                        assert!(
                            !frame
                                .hits
                                .iter()
                                .any(|(action, _)| *action == MenuAction::SelectProfileTab(tab)),
                            "the active tab does not offer a redundant navigation action"
                        );
                        dressing_room_reached |= frame.hits.iter().any(|(action, bounds)| {
                            *action == MenuAction::Navigate(MenuScreen::DressingRoom)
                                && bounds.width() > 0.0
                                && bounds.height() > 0.0
                        });
                        let areas = screen.take_paint().unwrap().scrolls;
                        let body = areas
                            .iter()
                            .find(|area| area.key == "profile_body")
                            .unwrap();
                        assert!(body.viewport.width() > 0.0 && body.viewport.height() > 0.0);
                        if dressing_room_reached {
                            break;
                        }
                        let point = crate::test_support::centre(body.viewport);
                        let delta = -body.viewport.height() * 0.5;
                        scrolls.set_areas(areas);
                        assert!(
                            scrolls.wheel(point, delta, true),
                            "Profile action is reachable by scrolling"
                        );
                    }
                    assert!(
                        dressing_room_reached,
                        "Profile keeps Dressing Room reachable"
                    );
                }
            }
        }
    }
}

/// Paints a real menu route with unchanged gameplay metrics and no installed assets.
fn paint(
    screen: &mut OreUiScreen,
    view: &MenuView,
    metrics: TextMetrics,
    size: [f32; 2],
) -> Painted {
    paint_with_scrolls(screen, view, metrics, size, &mut MenuScrolls::default())
}

/// Paints a route with the offsets produced by actual scrolling of its previous frame.
fn paint_with_scrolls(
    screen: &mut OreUiScreen,
    view: &MenuView,
    metrics: TextMetrics,
    size: [f32; 2],
    scrolls: &mut MenuScrolls<MenuAction>,
) -> Painted {
    let font = fixture_font();
    let mut layouts = TextLayoutCache::new(64, 1024 * 1024);
    let (mut nodes, mut next) = (Vec::new(), 1);
    let artwork = HashMap::new();
    let mut frame = PaintFrame {
        layouts: &mut layouts,
        font: &font,
        metrics,
        solid_page: 0,
        safe_area: SafeArea::ZERO,
        content: size,
        translate: &|_| None,
        language: [0; 3],
        now: 0.0,
        nodes: &mut nodes,
        next: &mut next,
        renderers: &mut [],
    };
    let hits = if view.dialog == Some(launcher::menu::MenuDialog::Accounts) {
        screen
            .paint_accounts(view, size, &artwork, scrolls, &mut frame)
            .unwrap()
    } else {
        screen
            .paint(
                MenuInput {
                    view,
                    size,
                    portrait: None,
                    translate: &|_| None,
                    artwork: &artwork,
                    title_artwork: None,
                    scrolls,
                    geometry: MenuGeometry::default(),
                },
                &mut frame,
            )
            .unwrap()
            .hits
    };
    // The OreUI adjustment must never leak into gameplay's frame metrics.
    assert_eq!(frame.metrics.gui_scale, metrics.gui_scale);
    assert_eq!(frame.metrics.scale, metrics.scale);
    Painted { nodes, hits }
}

#[test]
fn extra_large_progress_keeps_status_and_cancel_inside_small_windows() {
    for enabled in [0, 1] {
        for size in [ui::MIN_WINDOW_LOGICAL_SIZE, [400.0, 250.0], [752.0, 500.0]] {
            for dpi in [1.0, 2.0] {
                let mut view = MenuView::new(true, "Fixture".into());
                Arc::make_mut(&mut view.settings_options)
                    .set(option_index(EXTRA_LARGE_UI), enabled);
                view.local.progress = Some(launcher::local_worlds::Progress {
                    stage: launcher::local_worlds::Stage::DownloadingServer,
                    fraction: Some(0.5),
                    detail: "World download status ".repeat(160),
                });
                let metrics = TextMetrics::for_viewport(
                    size.map(|value| (value * dpi) as u32),
                    DpiScale::new(dpi).unwrap(),
                    Some(2),
                );
                let mut screen = OreUiScreen::default();
                let frame = paint(&mut screen, &view, metrics, size);
                let cancel = frame
                    .hits
                    .iter()
                    .find_map(|(action, bounds)| {
                        (*action == MenuAction::LocalWorld(launcher::menu::LocalWorldAction::Back))
                            .then_some(*bounds)
                    })
                    .expect("download progress keeps its Cancel target");
                assert!(cancel.width() > 0.0 && cancel.height() > 0.0);
                assert!(cancel.min().x() >= 0.0 && cancel.min().y() >= 0.0);
                assert!(cancel.max().x() <= size[0] && cancel.max().y() <= size[1]);
                let areas = screen.take_paint().unwrap().scrolls;
                let status = areas
                    .iter()
                    .find(|area| area.key == "loading_status")
                    .unwrap();
                assert!(status.viewport.width() > 0.0 && status.viewport.height() > 0.0);
                assert!(status.viewport.max().y() <= cancel.min().y());
                assert!(
                    status.max > 0.0,
                    "long status remains scrollable: {size:?}, DPI {dpi}, enabled {enabled}, GUI {}, viewport {:?}, max {}",
                    metrics.gui_scale,
                    status.viewport,
                    status.max
                );
            }
        }
    }
}

#[test]
fn progress_text_that_fits_does_not_create_a_scroll_range() {
    let size = [752.0, 500.0];
    let mut view = MenuView::new(true, "Fixture".into());
    view.local.progress = Some(launcher::local_worlds::Progress {
        stage: launcher::local_worlds::Stage::DownloadingServer,
        fraction: Some(0.5),
        detail: "World download status ".repeat(40),
    });
    let metrics = TextMetrics::for_viewport(
        size.map(|value| (value * 2.0) as u32),
        DpiScale::new(2.0).unwrap(),
        Some(2),
    );
    let mut screen = OreUiScreen::default();
    paint(&mut screen, &view, metrics, size);
    let areas = screen.take_paint().unwrap().scrolls;
    let status = areas
        .iter()
        .find(|area| area.key == "loading_status")
        .unwrap();
    assert_eq!(status.max, 0.0, "fitting status text needs no scroll range");
}

#[test]
fn extra_large_accounts_keeps_actions_and_focused_accounts_inside_small_windows() {
    for enabled in [0, 1] {
        for size in [ui::MIN_WINDOW_LOGICAL_SIZE, [400.0, 250.0], [752.0, 500.0]] {
            for dpi in [1.0, 2.0] {
                let mut view = crate::test_support::account_manager_view();
                view.dialog = Some(launcher::menu::MenuDialog::Accounts);
                view.feeds.accounts = (0..30)
                    .map(|index| launcher::accounts::AccountProfile {
                        id: index.to_string(),
                        gamertag: format!("Fixture{index}"),
                        picture_path: None,
                    })
                    .collect();
                view.focused_action = Some(MenuAction::SwitchAccount(29));
                Arc::make_mut(&mut view.settings_options)
                    .set(option_index(EXTRA_LARGE_UI), enabled);
                let metrics = TextMetrics::for_viewport(
                    size.map(|value| (value * dpi) as u32),
                    DpiScale::new(dpi).unwrap(),
                    Some(2),
                );
                let mut screen = OreUiScreen::default();
                let mut scrolls = MenuScrolls::default();
                let frame = paint_with_scrolls(&mut screen, &view, metrics, size, &mut scrolls);
                for action in [
                    MenuAction::AddAccount,
                    MenuAction::Navigate(MenuScreen::Profile),
                    MenuAction::DismissDialog,
                    MenuAction::SwitchAccount(29),
                ] {
                    let bounds = frame
                        .hits
                        .iter()
                        .find_map(|(at, bounds)| (*at == action).then_some(*bounds))
                        .expect("modal actions and the focused account stay reachable");
                    assert!(bounds.width() > 0.0 && bounds.height() > 0.0);
                    assert!(bounds.min().x() >= 0.0 && bounds.min().y() >= 0.0);
                    assert!(
                        bounds.max().x() <= size[0] && bounds.max().y() <= size[1],
                        "{action:?}: {bounds:?} at {size:?}, DPI {dpi}, enabled {enabled}"
                    );
                }
                assert!(
                    scrolls
                        .offsets()
                        .get(crate::modal::SCROLL)
                        .copied()
                        .unwrap_or(0.0)
                        > 0.0,
                    "the last account is revealed through actual focus scrolling"
                );
            }
        }
    }
}

#[test]
fn extra_large_add_server_keeps_fields_and_actions_reachable_in_small_windows() {
    for enabled in [0, 1] {
        for size in [[400.0, 250.0], ui::MIN_WINDOW_LOGICAL_SIZE, [752.0, 500.0]] {
            for dpi in [1.0, 2.0] {
                let mut view = MenuView::new(true, "Fixture".into());
                view.screen = MenuScreen::AddServer;
                view.name = "Fixture".into();
                view.address = "fixture.invalid".into();
                Arc::make_mut(&mut view.settings_options)
                    .set(option_index(EXTRA_LARGE_UI), enabled);
                let metrics = TextMetrics::for_viewport(
                    size.map(|value| (value * dpi) as u32),
                    DpiScale::new(dpi).unwrap(),
                    Some(2),
                );
                let mut screen = OreUiScreen::default();
                let mut scrolls = MenuScrolls::default();
                let mut reached = [false; 3];
                for _ in 0..32 {
                    let frame = paint_with_scrolls(&mut screen, &view, metrics, size, &mut scrolls);
                    for action in [
                        MenuAction::AddBack,
                        MenuAction::AddSave,
                        MenuAction::AddSaveConnect,
                    ] {
                        let bounds = frame
                            .hits
                            .iter()
                            .find_map(|(at, bounds)| (*at == action).then_some(*bounds))
                            .expect("the close and save actions stay visible");
                        assert!(bounds.min().x() >= 0.0 && bounds.min().y() >= 0.0);
                        assert!(bounds.max().x() <= size[0] && bounds.max().y() <= size[1]);
                    }
                    let areas = screen.take_paint().unwrap().scrolls;
                    let fields = areas
                        .iter()
                        .find(|area| area.key == "add_server_fields")
                        .expect("server fields have a scroll viewport");
                    assert!(fields.viewport.width() > 0.0 && fields.viewport.height() > 0.0);
                    for (index, action) in [
                        MenuAction::AddName,
                        MenuAction::AddAddress,
                        MenuAction::AddPort,
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        reached[index] |= frame.hits.iter().any(|(at, bounds)| {
                            *at == action && bounds.width() > 0.0 && bounds.height() > 0.0
                        });
                    }
                    if reached.into_iter().all(|value| value) {
                        break;
                    }
                    let point = crate::test_support::centre(fields.viewport);
                    let delta = -fields.viewport.height() * 0.5;
                    scrolls.set_areas(areas);
                    assert!(
                        scrolls.wheel(point, delta, true),
                        "each field must be reachable: {size:?}, DPI {dpi}, enabled {enabled}"
                    );
                }
                assert!(
                    reached.into_iter().all(|value| value),
                    "each field must be reachable: {size:?}, DPI {dpi}, enabled {enabled}"
                );
            }
        }
    }
}

/// Returns the actual navigation hit region the pointer uses.
fn back(frame: &Painted) -> UiRect {
    frame
        .hits
        .iter()
        .find_map(|(action, bounds)| (*action == MenuAction::AddBack).then_some(*bounds))
        .expect("menu route has a back control")
}

/// Measures the painted title's height using its laid-out node bounds.
fn title_height(frame: &Painted, title: &str) -> f32 {
    frame
        .nodes
        .iter()
        .find_map(|node| match node.visual() {
            ui::UiVisual::Text { layout, .. }
                if layout
                    .glyphs()
                    .iter()
                    .map(|glyph| glyph.codepoint)
                    .collect::<String>()
                    == title =>
            {
                Some(node.bounds().height())
            }
            _ => None,
        })
        .expect("menu title is painted")
}

#[test]
fn extra_large_ui_resizes_real_menu_text_controls_and_pointer_targets() {
    for (physical, gui) in [([1280, 720], 2), ([1920, 1080], 4)] {
        for dpi in [1.0, 2.0] {
            for (route, title) in [
                (MenuScreen::Settings, "SETTINGS"),
                (MenuScreen::Profile, "Your Profile"),
            ] {
                let mut view = crate::test_support::settings_view("video_forced_index");
                view.screen = route;
                let index = option_index(EXTRA_LARGE_UI);
                let metrics =
                    TextMetrics::for_viewport(physical, DpiScale::new(dpi).unwrap(), Some(gui));
                let size = physical.map(|value| value as f32 / dpi);
                let mut screen = OreUiScreen::default();
                let normal = paint(&mut screen, &view, metrics, size);
                assert!(Arc::make_mut(&mut view.settings_options).set(index, 1));
                // Loading saved settings exercises the same state as a fresh client session.
                view.settings_options = Arc::new(
                    SettingsOptions::decode(&serde_json::to_vec(&view.settings_options).unwrap())
                        .unwrap(),
                );
                let large = paint(&mut screen, &view, metrics, size);
                let factor = (f32::from(gui) + 1.0) / f32::from(gui);
                assert!(
                    (back(&large).width() / back(&normal).width() - factor).abs() < 0.001,
                    "extra large UI must resize actual controls: {route:?}, DPI {dpi}"
                );
                assert!(
                    (title_height(&large, title) / title_height(&normal, title) - factor).abs()
                        < 0.01,
                    "extra large UI must resize text with its controls"
                );
                let point = UiPoint::new(
                    (back(&normal).max().x() + back(&large).max().x()) * 0.5,
                    back(&normal).height() * 0.5,
                )
                .unwrap();
                assert!(!back(&normal).contains(point));
                assert!(
                    back(&large).contains(point),
                    "enlarged controls need enlarged pointer targets"
                );
                assert!(Arc::make_mut(&mut view.settings_options).set(index, 0));
                let restored = paint(&mut screen, &view, metrics, size);
                assert_eq!(back(&restored), back(&normal));
                assert_eq!(title_height(&restored, title), title_height(&normal, title));
            }
        }
    }
}
