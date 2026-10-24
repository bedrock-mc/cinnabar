//! Presentation attachment after actual fixed-account validation; no authentication shortcut proof.

use super::MenuRuntime;
use launcher::menu::{MenuScreen, auth::AuthState, view::MenuProfile};
use launcher_host::{
    auth::test_support::native_state, native_account::test_support::friend::Fixture,
};

/// Attaches only the same sealed acquired account after its real selected-generation validation.
/// Original AuthSupervisor restoration is outside this presentation fixture's scope.
/// The ordinary catalogue worker still must obtain and apply the actual opaque friend rows.
pub async fn attach(menu: &mut MenuRuntime, fixture: &Fixture) -> std::io::Result<()> {
    if menu.auth_process.is_some() || menu.account_change_pending() {
        return Err(std::io::Error::other(
            "friend presenter requires an unused menu",
        ));
    }
    let presentation = fixture.presentation().await?;
    if !presentation.is_current() {
        return Err(std::io::Error::other("friend presenter identity retired"));
    }
    let profile = menu
        .native_catalog
        .install_fixture(presentation, &menu.layout.auth_cache())?;
    menu.feeds.profile = MenuProfile {
        unavailable: false,
        xuid: profile.id.clone(),
        gamertag: profile.gamertag.clone(),
        picture_path: profile.picture_path.unwrap_or_default(),
        ..MenuProfile::unavailable()
    };
    menu.feeds.account_active_id = Some(profile.id);
    menu.auth_process = Some(native_state(AuthState::Authenticated));
    menu.auth_attempted = true;
    menu.screen = MenuScreen::Servers;
    menu.visible = true;
    Ok(())
}

/// Polls the same original frame catalogue owner; no service call is added by this observation.
pub fn poll(menu: &mut MenuRuntime) {
    menu.poll_native_catalog(std::time::Instant::now());
}
