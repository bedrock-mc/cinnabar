//! Drives account, world and settings services from the menu.
use super::*;
/// Drives the launcher's own services: catalog, saves, settings, the account core and local worlds.
#[allow(clippy::too_many_arguments)]
pub(crate) fn drive_menu_services(
    mut commands: Commands,
    mut menu: ResMut<MenuRuntime>,
    client_blob_cache: Res<crate::app::ClientBlobCacheOwner>,
    player_runtime: Res<crate::player_runtime::PlayerRuntime>,
    mut client_world: ResMut<ClientWorld>,
    mut runtime: ResMut<UiRuntime>,
    launcher: Option<ResMut<LauncherCoreSlot>>,
    launcher_account: Option<ResMut<launcher_account::LauncherAccount>>,
    mut local_worlds: Option<ResMut<crate::local_worlds::LocalWorlds>>,
    audio_settings: Option<ResMut<crate::audio::AudioSettings>>,
    settings: Option<ResMut<crate::settings_runtime::RuntimeSettings>>,
    antialiasing: Option<Res<client_presentation::camera::antialiasing::CameraAntiAliasingSupport>>,
    mut local_skin: Option<ResMut<crate::player_skin::LocalPlayerSkin>>,
    network: Option<Res<crate::runtime::network::NetworkHandle>>,
) {
    #[cfg(feature = "developer-control")]
    if menu.fixture_active() {
        return;
    }
    menu.poll_dressing_room(
        local_skin.as_deref_mut(),
        &mut client_world,
        network.as_deref(),
        runtime.session_id(),
    );
    menu.poll_catalog(launcher_account.is_some());
    menu.poll_saves();
    menu.poll_accounts();
    menu.sync_audio_settings(audio_settings);
    if let Some(support) = antialiasing {
        menu.sync_anti_aliasing_support(support.0);
    }
    menu.sync_user_settings(settings);
    menu.sync_language(&mut runtime);
    let in_session = client_world.stream.is_some();
    if let Some(mut slot) = launcher {
        // Remote direct sessions have a separate game core. Local worlds use
        // the account core, so sign-in must not restart it during local play.
        let idle = launcher_core::account_core_idle(
            menu.is_launcher(),
            menu.is_connecting(),
            in_session,
            menu.local_world_joined,
        );
        slot.drive(
            &mut commands,
            &mut menu,
            idle,
            client_blob_cache.enables_upstream_client_cache(),
            local_worlds.as_deref_mut(),
        );
    }
    if std::mem::take(&mut menu.accounts.skip_control) {
        menu.forget_launcher_trust();
        return;
    }
    match launcher_account {
        Some(mut account) => {
            account.set_xbox_presence(xbox_presence::state(
                in_session,
                player_runtime.facts.world_default_game_mode(),
                menu.presence_address(),
                menu.presence_is_featured(),
            ));
            menu.sync_account_control(&mut *account);
        }
        None => {
            menu.forget_launcher_trust();
            menu.sign_out_locally();
        }
    }
    if let Some(worlds) = local_worlds.as_deref_mut() {
        menu.sync_local_worlds(worlds, in_session);
    }
}
