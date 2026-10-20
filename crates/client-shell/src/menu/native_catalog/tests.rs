use super::*;
use launcher::menu::view::{MenuFriendCard, MenuRealmCard};
use launcher_host::account_control::{AccountControl, AccountEvent};

/// Supplies only legacy account feeds; no process, HTTP or credentials are involved.
struct Legacy;
impl AccountControl for Legacy {
    fn account_status(&mut self) -> Option<AuthState> {
        None
    }
    fn realms(&mut self) -> Option<Vec<MenuRealmCard>> {
        Some(vec![realm("legacy")])
    }
    fn friends(&mut self) -> Option<Vec<MenuFriendCard>> {
        Some(vec![friend("legacy")])
    }
    fn sign_out(&mut self) -> bool {
        false
    }
    fn poll_event(&mut self) -> Option<AccountEvent> {
        None
    }
}

/// Creates an ordinary existing menu model for ownership assertions.
fn realm(name: &str) -> MenuRealmCard {
    serde_json::from_value(serde_json::json!({"name":name,"state":"OPEN","target":format!("{}7",bridge::contract::REALM_TARGET_PREFIX)})).unwrap()
}

/// Reuses the production card projection for a synthetic listed world.
pub(super) fn friend(world: &str) -> MenuFriendCard {
    launcher::menu::view::CatalogFriend {
        gamertag: "Synthetic friend".into(),
        xuid: "456".into(),
        world_name: world.into(),
        members: 1,
        max_members: 2,
    }
    .into()
}

/// Native results own both feeds, survive independent section failures and share the account with joins.
#[test]
fn native_catalog_owns_both_feeds_and_shares_the_selected_account_with_joins() {
    let mut menu = MenuRuntime::new(true, 2, "Synthetic".into()).with_native_sign_in(true);
    menu.feeds.account_active_id = Some("123".into());
    let path = menu.layout.auth_cache();
    let first = menu.native_account_for(path.clone());
    menu.apply_native_catalog(CatalogCards {
        realms: Ok(vec![realm("native")]),
        friends: Ok(vec![friend("native").into()]),
    });
    menu.sync_account_control(&mut Legacy);
    assert_eq!(menu.realms[0].name, "native");
    assert_eq!(menu.friends[0].world_name, "native");
    menu.apply_native_catalog(CatalogCards {
        realms: Err(std::io::ErrorKind::TimedOut.into()),
        friends: Ok(vec![friend("new native").into()]),
    });
    assert_eq!(menu.realms[0].name, "native");
    assert_eq!(menu.friends[0].world_name, "new native");
    assert_eq!(menu.catalog_message.as_deref(), Some(FAILURE_MESSAGE));
    menu.apply_native_catalog(CatalogCards {
        realms: Ok(vec![realm("new native")]),
        friends: Err(std::io::ErrorKind::TimedOut.into()),
    });
    assert_eq!(menu.realms[0].name, "new native");
    assert_eq!(menu.friends[0].world_name, "new native");

    assert!(Arc::ptr_eq(&first, &menu.native_account_for(path.clone())));
    menu.native_catalog.refresh();
    assert!(
        Arc::ptr_eq(&first, &menu.native_account_for(path.clone())),
        "refresh replaced the account/session owner"
    );
    menu.activate(MenuAction::PlayFriend(0));
    assert!(
        menu.intents.join.is_none(),
        "listing enabled an unsupported native join"
    );
    assert!(
        menu.message
            .as_deref()
            .is_some_and(|message| message.contains("not available"))
    );
    menu.feeds.account_active_id = Some("456".into());
    let next = menu.native_account_for(path);
    assert!(!Arc::ptr_eq(&first, &next));
    assert!(
        menu.realms.is_empty() && menu.friends.is_empty(),
        "the previous selected account's catalog survived"
    );

    let mut go = MenuRuntime::new(true, 2, "Synthetic".into());
    go.sync_account_control(&mut Legacy);
    assert_eq!(go.realms[0].name, "legacy");
    assert_eq!(go.friends[0].world_name, "legacy");
}

mod retirement;
