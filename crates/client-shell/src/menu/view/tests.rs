use crate::menu::{
    LocalWorldCard, MenuFriendCard, MenuRealmCard, MenuRuntime, MenuServerCard, SavedServer,
};
use launcher::menu::{MenuAction, MenuView};

/// Creates populated owners without starting any catalog or account worker.
fn menu() -> MenuRuntime {
    let mut menu = MenuRuntime::new(true, 2, "Fixture".into());
    menu.catalog_started = true;
    menu.servers = vec![SavedServer {
        name: "Saved".into(),
        address: "saved.example.test:19132".into(),
        favorite: false,
        last_joined_unix: 0,
    }]
    .into();
    menu.featured = vec![MenuServerCard {
        name: "Featured".into(),
        address: "featured.example.test:19132".into(),
        caption: "Games".into(),
        image_path: String::new(),
        icon: None,
    }]
    .into();
    menu.realms = vec![MenuRealmCard {
        name: "Realm".into(),
        state: "OPEN".into(),
        target: "realm_id/fixture".into(),
        address: String::new(),
        owner: "Fixture".into(),
        online_players: 1,
        max_players: 10,
        days_left: 1,
        expired: false,
        member: true,
    }]
    .into();
    menu.friends = vec![MenuFriendCard {
        gamertag: "Friend".into(),
        world_name: "World".into(),
        members: "1/8".into(),
        xuid: "fixture-friend".into(),
        max_members: 8,
    }]
    .into();
    menu.set_local_worlds(vec![LocalWorldCard {
        name: "Local".into(),
        ..Default::default()
    }]);
    menu
}

/// Checks that these populated owners reuse their entry allocations across views.
fn assert_shared(first: &MenuView, second: &MenuView) {
    assert_eq!(first.servers.as_ptr(), second.servers.as_ptr());
    assert_eq!(first.featured.as_ptr(), second.featured.as_ptr());
    assert_eq!(first.realms.as_ptr(), second.realms.as_ptr());
    assert_eq!(first.friends.as_ptr(), second.friends.as_ptr());
    assert_eq!(first.local_worlds.as_ptr(), second.local_worlds.as_ptr());
}

#[test]
fn repeated_views_reuse_every_list_while_interaction_state_changes() {
    let mut menu = menu();
    let original = menu.view();
    for revision in 1..=240 {
        menu.hovered = Some(MenuAction::PlaySaved(0));
        menu.pressed = Some(MenuAction::PlaySaved(0));
        menu.caret_revision = revision;
        let current = menu.view();
        assert_shared(&original, &current);
        assert_eq!(current.hovered, Some(MenuAction::PlaySaved(0)));
        assert_eq!(current.pressed, Some(MenuAction::PlaySaved(0)));
        assert_eq!(current.caret.revision, revision);
    }
    assert_eq!(original.hovered, None);
    assert_eq!(original.caret.revision, 0);
}

#[test]
fn settings_views_retain_interactions_without_including_live_pointer_capture() {
    let mut menu = menu();
    menu.enter(launcher::menu::MenuScreen::Settings);
    let original = menu.view();
    menu.settings_input.slider_drag =
        Some(launcher::menu::settings_options::SettingId::Gamma.index() as u16);
    assert_eq!(original, menu.view());

    menu.activate_from_navigation(MenuAction::SettingsAdvancedGraphics);
    let changed = menu.view();
    assert!(!original.settings.advanced_graphics);
    assert!(changed.settings.advanced_graphics);
    assert!(changed.settings.control_activation_navigation);
    assert_ne!(original, changed);
    assert_shared(&original, &changed);
    assert!(std::sync::Arc::ptr_eq(
        &original.settings_options,
        &changed.settings_options,
    ));

    let mut detached = changed.clone();
    detached.settings.advanced_graphics = false;
    detached.settings.key_remap = Some(0);
    assert_eq!(changed, menu.view());
    assert_ne!(detached, menu.view());
}

#[test]
fn saved_server_actions_publish_only_the_changed_list_and_retain_prior_views() {
    let mut menu = menu();
    let original = menu.view();
    menu.activate(MenuAction::ToggleFavorite(0));
    let favorite = menu.view();
    assert!(!original.servers.shares_storage(&favorite.servers));
    assert!(!original.servers[0].favorite);
    assert!(favorite.servers[0].favorite);
    assert!(original.featured.shares_storage(&favorite.featured));
    assert!(original.realms.shares_storage(&favorite.realms));
    assert!(original.friends.shares_storage(&favorite.friends));
    assert!(original.local_worlds.shares_storage(&favorite.local_worlds));
    menu.activate(MenuAction::ConfirmRemoveSaved(0));
    assert!(menu.view().servers.is_empty());
    assert_eq!(favorite.servers[0].name, "Saved");
    assert!(favorite.servers[0].favorite);
}

#[test]
fn local_world_deliveries_retain_equal_data_but_publish_new_thumbnail_storage() {
    use launcher::local_worlds::world::WorldThumbnail;
    use std::sync::Arc;

    let mut menu = menu();
    let original = menu.view();
    menu.set_local_worlds(original.local_worlds.to_vec());
    assert_shared(&original, &menu.view());

    let mut worlds = original.local_worlds.to_vec();
    worlds[0].thumbnail = Some(WorldThumbnail::new("fixture", Arc::from([1_u8, 2, 3])));
    menu.set_local_worlds(worlds);
    let first_image = menu.view();
    assert!(original.local_worlds[0].thumbnail.is_none());
    assert!(
        !original
            .local_worlds
            .shares_storage(&first_image.local_worlds)
    );
    menu.set_local_worlds(first_image.local_worlds.to_vec());
    assert_shared(&first_image, &menu.view());

    let mut worlds = first_image.local_worlds.to_vec();
    worlds[0].thumbnail = Some(WorldThumbnail::new("fixture", Arc::from([4_u8, 5, 6])));
    menu.set_local_worlds(worlds);
    let second_image = menu.view();
    assert!(
        !first_image
            .local_worlds
            .shares_storage(&second_image.local_worlds)
    );
    let first = first_image.local_worlds[0].thumbnail.as_ref().unwrap();
    let second = second_image.local_worlds[0].thumbnail.as_ref().unwrap();
    assert_eq!(first.key, second.key);
    assert_eq!(first.image.as_ref(), [1, 2, 3]);
    assert_eq!(second.image.as_ref(), [4, 5, 6]);
    menu.set_local_worlds(Vec::new());
    assert!(menu.view().local_worlds.is_empty());
    assert_eq!(second_image.local_worlds.len(), 1);
}

#[test]
fn mutating_a_published_view_cannot_change_the_owner_or_another_view() {
    let menu = menu();
    let original = menu.view();
    let mut detached = original.clone();
    detached.featured[0].image_path = "fixture-artwork.png".into();
    detached.realms[0].online_players += 1;
    detached.friends.clear();
    assert_shared(&original, &menu.view());
    assert!(original.featured[0].image_path.is_empty());
    assert_eq!(original.realms[0].online_players, 1);
    assert_eq!(original.friends.len(), 1);
    assert_ne!(original.featured, detached.featured);
    assert_ne!(original.realms, detached.realms);
    assert_ne!(original.friends, detached.friends);
}

#[test]
fn artwork_and_icon_edits_publish_featured_cards_without_rewriting_prior_views() {
    let mut menu = menu();
    let original = menu.view();
    launcher_host::catalog_art::apply(
        &mut menu.featured,
        &[(
            original.featured[0].address.clone(),
            "fixture-artwork.png".into(),
        )],
    );
    let artwork = menu.view();
    assert!(original.featured[0].image_path.is_empty());
    assert_eq!(artwork.featured[0].image_path, "fixture-artwork.png");
    assert!(original.servers.shares_storage(&artwork.servers));
    assert!(original.realms.shares_storage(&artwork.realms));
    assert!(original.friends.shares_storage(&artwork.friends));
    assert!(original.local_worlds.shares_storage(&artwork.local_worlds));
    let icon = ui::IconRef {
        color_mask: None,
        page: 1,
        uv: [0, 0, 16, 16],
        glint: false,
    };
    menu.featured[0].icon = Some(icon);
    let with_icon = menu.view();
    assert_eq!(artwork.featured[0].icon, None);
    assert_eq!(with_icon.featured[0].icon, Some(icon));
    assert_ne!(artwork.featured, with_icon.featured);
}
