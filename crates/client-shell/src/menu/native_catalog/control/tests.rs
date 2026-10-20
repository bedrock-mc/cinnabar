use super::*;
use crate::menu::native_catalog::tests::friend;
use launcher_host::auth::test_support::native_state;

/// Supplies display-only catalog rows to exercise guards, never a verified service selection.
fn menu() -> MenuRuntime {
    let layout = launcher::test_support::scratch("native-friend-control");
    let mut menu = MenuRuntime::new_with_layout(
        true,
        Some(2),
        "Fixture".into(),
        layout,
        crate::player_skin::LocalPlayerSkin::generated_default("Fixture"),
    )
    .with_native_sign_in(true);
    menu.auth_process = Some(native_state(AuthState::Authenticated));
    menu.feeds.account_active_id = Some("123".into());
    menu.native_account_for(menu.layout.auth_cache());
    replace_friends(&mut menu, "First world");
    menu
}

/// Uses the existing publication owner, whose successful rows revoke prior index observations.
fn replace_friends(menu: &mut MenuRuntime, name: &str) {
    menu.apply_native_catalog(CatalogCards {
        realms: Ok(Vec::new()),
        friends: Ok(vec![friend(name).into()]),
    });
}

/// Copies only the observations an actual control caller receives.
fn choice(report: &NativeFriendsReport) -> NativeFriendChoice {
    NativeFriendChoice {
        revision: report.revision,
        index: report.worlds[0].index,
        expected_xuid: report.worlds[0].xuid.clone(),
    }
}

#[test]
fn native_friend_control_rejects_stale_indices_and_display_only_rows() {
    let mut menu = menu();
    let first = menu.developer_native_friends(None).unwrap();
    assert!(!first.queued && !first.worlds[0].selectable);
    assert_eq!(first.worlds[0].world_name, "First world");
    let selected = choice(&first);
    assert!(
        menu.developer_native_friends(Some(selected.clone()))
            .is_err()
    );
    assert!(menu.take_native_friend_intent().is_none());

    let mut wrong = selected.clone();
    wrong.expected_xuid = "789".into();
    assert_eq!(
        menu.developer_native_friends(Some(wrong)).unwrap_err(),
        STALE
    );
    let mut wrong = selected.clone();
    wrong.index = 1;
    assert_eq!(
        menu.developer_native_friends(Some(wrong)).unwrap_err(),
        STALE
    );

    replace_friends(&mut menu, "Replacement world");
    assert_eq!(
        menu.developer_native_friends(Some(selected)).unwrap_err(),
        STALE
    );
    let current = menu.developer_native_friends(None).unwrap();
    menu.apply_native_catalog(CatalogCards {
        realms: Ok(Vec::new()),
        friends: Err(std::io::ErrorKind::TimedOut.into()),
    });
    let retained = menu.developer_native_friends(None).unwrap();
    assert_eq!(retained.revision, current.revision);
    assert_eq!(retained.worlds[0].world_name, "Replacement world");
    menu.native_catalog.refresh();
    assert_eq!(
        menu.developer_native_friends(Some(choice(&retained)))
            .unwrap_err(),
        STALE
    );
    assert!(menu.take_native_friend_intent().is_none());
}

#[test]
fn native_friend_control_uses_only_the_current_real_account_mode() {
    let mut menu = menu();
    let report = menu.developer_native_friends(None).unwrap();
    menu.feeds.account_active_id = Some("789".into());
    assert_eq!(
        menu.developer_native_friends(Some(choice(&report)))
            .unwrap_err(),
        UNAVAILABLE
    );
    menu.feeds.account_active_id = Some("123".into());
    menu.feeds.account_adding = true;
    assert_eq!(
        menu.developer_native_friends(None).unwrap_err(),
        UNAVAILABLE
    );
    menu.feeds.account_adding = false;
    menu.presentation_accounts = true;
    assert_eq!(
        menu.developer_native_friends(None).unwrap_err(),
        UNAVAILABLE
    );
    menu.presentation_accounts = false;
    menu.sign_in_fixture = Some(developer_control::protocol::SignInFixtureState::Success);
    assert_eq!(
        menu.developer_native_friends(None).unwrap_err(),
        UNAVAILABLE
    );
    menu.sign_in_fixture = None;
    menu.native_sign_in = false;
    assert_eq!(
        menu.developer_native_friends(None).unwrap_err(),
        UNAVAILABLE
    );
    assert!(menu.take_native_friend_intent().is_none());
}
