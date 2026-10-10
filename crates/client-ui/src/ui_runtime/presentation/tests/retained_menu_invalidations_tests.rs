use super::*;
use crate::menu::{MenuScreen, MenuServerCard, MenuView, PingInfo, SavedServer};
use launcher::menu::server_list::{ServerGroup, ServerListAction};

/// Two featured and saved entries expose selected and unrelated ping states.
pub(super) fn servers_view() -> MenuView {
    let mut view = MenuView::new(true, "Fixture".into());
    view.screen = MenuScreen::Servers;
    view.featured = ["featured", "creator"]
        .into_iter()
        .map(|name| MenuServerCard {
            name: name.into(),
            address: format!("{name}.test:19132"),
            caption: String::new(),
            image_path: String::new(),
            icon: None,
        })
        .collect();
    view.servers = ["saved", "other"]
        .into_iter()
        .map(|name| SavedServer {
            name: name.into(),
            address: format!("{name}.test:19132"),
            favorite: false,
            last_joined_unix: 0,
        })
        .collect();
    view.feeds.details.insert(
        view.featured[1].address.clone(),
        crate::menu::ServerDetails {
            group: "creator".into(),
            ..Default::default()
        },
    );
    view
}

/// Assigns a pong without changing the server identities or current selection.
pub(super) fn pong(view: &mut MenuView, address: String, online: bool) {
    view.feeds.pings.insert(
        address,
        PingInfo {
            online,
            ..Default::default()
        },
    );
}

#[test]
fn selected_ping_animation_follows_default_explicit_and_collapsed_selection() {
    let mut view = servers_view();
    let animated = super::super::forms::oreui::animated_server_details;
    assert!(
        animated(&view),
        "the default featured detail has a pending ping"
    );
    let featured = view.featured[0].address.clone();
    pong(&mut view, featured.clone(), true);
    assert!(
        !animated(&view),
        "unselected pending servers cannot block retention"
    );
    pong(&mut view, featured, false);
    assert!(
        animated(&view),
        "an offline pong still uses the looping icon"
    );
    view.feeds.select_saved(0);
    assert!(
        animated(&view),
        "explicit custom selection uses its missing pong"
    );
    let saved = view.servers[0].address.clone();
    pong(&mut view, saved.clone(), false);
    assert!(animated(&view));
    pong(&mut view, saved, true);
    assert!(
        !animated(&view),
        "unselected offline featured servers do not animate"
    );
    Arc::make_mut(&mut view.settings_options)
        .apply_server_list(ServerListAction::Toggle(ServerGroup::Saved));
    assert!(
        !animated(&view),
        "collapsed sections keep selected online details"
    );
    let saved = view.servers[0].address.clone();
    pong(&mut view, saved, false);
    assert!(
        animated(&view),
        "collapsed selected offline details still animate"
    );
    view.featured.clear();
    view.servers.clear();
    assert!(!animated(&view), "empty details have no ping animation");
}

#[test]
fn gathering_details_without_a_ping_strip_can_be_retained() {
    let mut view = servers_view();
    view.featured[0].address =
        format!("{}fixture", launcher::menu::view::EXPERIENCE_ADDRESS_PREFIX);
    assert!(!super::super::forms::oreui::animated_server_details(&view));
    view.featured.clear();
    assert!(
        super::super::forms::oreui::animated_server_details(&view),
        "custom fallback still paints its ping strip"
    );
}

/// Supplies synthetic ping cells so temporal output is tested without installed Mojang images.
fn install_ping_cells(presentation: &mut UiPresentationRuntime) {
    use crate::ui_runtime::oreui_assets::{
        OreUiImages, OreUiPage, OreUiSprite, SERVER_PING_IMAGES,
    };
    let sprites = std::collections::HashMap::from([(
        SERVER_PING_IMAGES[3].into(),
        OreUiSprite {
            page: 0,
            bounds: [0, 0, 6, 1],
        },
    )]);
    presentation
        .enable_oreui_originals(OreUiImages {
            pages: vec![OreUiPage {
                dimensions: [6, 1],
                pixels: vec![255; 24].into(),
            }],
            sprites: Arc::new(sprites),
            loading_frames: Default::default(),
            animations: Default::default(),
            source: None,
        })
        .unwrap();
}

/// Publishes the same menu snapshot path used by the application with a deterministic clock.
fn frame(presentation: &mut UiPresentationRuntime, view: &MenuView, clock: u64) -> UiRenderInput {
    presentation.set_menu_view(Some(view.clone()));
    presentation
        .build(
            &player_state::PlayerState::new(0),
            &UiRuntime::new(0),
            clock,
            [1280, 720],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap()
}

/// Advances finite entrance and control channels before checking retention.
fn settle(presentation: &mut UiPresentationRuntime, view: &MenuView, clock: u64) -> UiRenderInput {
    frame(presentation, view, clock);
    frame(presentation, view, clock + 500);
    frame(presentation, view, clock + 1000)
}

#[test]
fn missing_and_offline_selected_pings_keep_publishing_spinner_cells() {
    for saved in [false, true] {
        let Some(mut presentation) = super::super::forms::pack_harness::engine_presentation()
        else {
            eprintln!(
                "skipping missing_and_offline_selected_pings_keep_publishing_spinner_cells: missing local UI carrier (make assets)"
            );
            return;
        };
        install_ping_cells(&mut presentation);
        let mut view = servers_view();
        if saved {
            view.feeds.select_saved(0);
            let address = view.servers[0].address.clone();
            pong(&mut view, address, false);
        }
        settle(&mut presentation, &view, 0);
        let before = frame(&mut presentation, &view, 1500);
        let paints = presentation.oreui_paints;
        let after = frame(&mut presentation, &view, 1700);
        assert!(presentation.oreui_paints > paints);
        assert_ne!(
            before.revision, after.revision,
            "the selected spinner advances"
        );
    }
}

#[test]
fn retained_hover_and_settings_changes_repaint_then_settle() {
    let Some(mut presentation) = super::super::forms::pack_harness::engine_presentation() else {
        eprintln!(
            "skipping retained_hover_and_settings_changes_repaint_then_settle: missing local UI carrier (make assets)"
        );
        return;
    };
    let mut view = servers_view();
    let address = view.featured[0].address.clone();
    pong(&mut view, address, true);
    settle(&mut presentation, &view, 0);
    let paints = presentation.oreui_paints;
    view.hovered = Some(MenuAction::PlayAddServer);
    frame(&mut presentation, &view, 1500);
    assert!(
        presentation.oreui_paints > paints,
        "hover invalidates the saved output"
    );
    let hovered = settle(&mut presentation, &view, 1600);
    let paints = presentation.oreui_paints;
    let again = frame(&mut presentation, &view, 2700);
    assert_eq!(presentation.oreui_paints, paints);
    assert!(Arc::ptr_eq(&hovered.vertices, &again.vertices));
    let option = launcher::menu::settings_options::SETTINGS_OPTIONS
        .iter()
        .position(|option| option.name == launcher::menu::settings_options::OREUI_DARK_MODE)
        .unwrap();
    let next = 1 - view.settings_options.get(option);
    assert!(Arc::make_mut(&mut view.settings_options).set(option, next));
    let changed = frame(&mut presentation, &view, 2800);
    assert!(
        presentation.oreui_paints > paints,
        "settings invalidate the saved output"
    );
    assert_ne!(changed.revision, hovered.revision);
    let settled = settle(&mut presentation, &view, 2900);
    let paints = presentation.oreui_paints;
    let again = frame(&mut presentation, &view, 4000);
    assert_eq!(presentation.oreui_paints, paints);
    assert!(Arc::ptr_eq(&settled.vertices, &again.vertices));
}
