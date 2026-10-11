//! Drives account, world and settings services from the menu.
use crate::runtime::world::ClientWorld;
use bevy::prelude::*;
use client_shell::{
    menu::{LauncherCoreSlot, MenuRuntime, launcher_account},
    player_skin::LocalPlayerSkin,
};
use client_ui::ui_runtime::UiRuntime;
/// Drives the launcher's own services: catalog, saves, settings, the account core and local worlds.
#[allow(clippy::too_many_arguments)]
pub(crate) fn drive_menu_services(
    mut commands: Commands,
    mut menu: ResMut<MenuRuntime>,
    client_blob_cache: Res<crate::app::ClientBlobCacheOwner>,
    player_runtime: Res<client_state::PlayerRuntime>,
    mut client_world: ResMut<ClientWorld>,
    mut runtime: ResMut<UiRuntime>,
    launcher: Option<ResMut<LauncherCoreSlot>>,
    launcher_account: Option<ResMut<launcher_account::LauncherAccount>>,
    mut local_worlds: Option<ResMut<client_shell::local_worlds::LocalWorlds>>,
    audio_settings: Option<ResMut<client_audio::audio::AudioSettings>>,
    settings: Option<ResMut<client_state::RuntimeSettings>>,
    antialiasing: Option<Res<client_presentation::camera::antialiasing::CameraAntiAliasingSupport>>,
    mut local_skin: Option<ResMut<LocalPlayerSkin>>,
    network: Option<Res<crate::runtime::network::NetworkHandle>>,
) {
    #[cfg(feature = "developer-control")]
    if menu.fixture_active() {
        return;
    }
    let send = network
        .as_ref()
        .map(|network| move |generation, packet| network.send_settings_packet(generation, packet));
    menu.poll_dressing_room(
        local_skin.as_deref_mut(),
        client_world.stream.as_mut(),
        send.as_ref().map(|send| send as &dyn Fn(_, _) -> _),
        runtime.session_id(),
    );
    menu.poll_catalog(launcher_account.is_some());
    menu.poll_saves();
    menu.poll_accounts();
    sync_audio_settings(&menu, audio_settings);
    if let Some(support) = antialiasing {
        menu.sync_anti_aliasing_support(support.0);
    }
    menu.sync_user_settings(settings);
    menu.sync_language(&mut runtime);
    menu.drive_account_services(
        &mut commands,
        client_blob_cache.enables_upstream_client_cache(),
        &player_runtime,
        client_world.stream.is_some(),
        launcher,
        launcher_account,
        local_worlds.as_deref_mut(),
    );
}

use client_audio::audio::{AudioCategory, AudioSettings};
/// The sound section's mixer categories; text-to-speech persists without a mixer backend.
const VOLUME_SLIDERS: [(&str, Option<AudioCategory>); 11] = [
    (
        launcher::menu::settings_options::VOLUME_SETTINGS[0],
        Some(AudioCategory::Master),
    ),
    (
        launcher::menu::settings_options::VOLUME_SETTINGS[1],
        Some(AudioCategory::Music),
    ),
    (
        launcher::menu::settings_options::VOLUME_SETTINGS[2],
        Some(AudioCategory::Sound),
    ),
    (
        launcher::menu::settings_options::VOLUME_SETTINGS[3],
        Some(AudioCategory::Ambient),
    ),
    (
        launcher::menu::settings_options::VOLUME_SETTINGS[4],
        Some(AudioCategory::Blocks),
    ),
    (
        launcher::menu::settings_options::VOLUME_SETTINGS[5],
        Some(AudioCategory::Hostile),
    ),
    (
        launcher::menu::settings_options::VOLUME_SETTINGS[6],
        Some(AudioCategory::Neutral),
    ),
    (
        launcher::menu::settings_options::VOLUME_SETTINGS[7],
        Some(AudioCategory::Players),
    ),
    (
        launcher::menu::settings_options::VOLUME_SETTINGS[8],
        Some(AudioCategory::Records),
    ),
    (
        launcher::menu::settings_options::VOLUME_SETTINGS[9],
        Some(AudioCategory::Weather),
    ),
    (launcher::menu::settings_options::VOLUME_SETTINGS[10], None),
];

/// Applies the saved sound values to the live mixer.
pub(crate) fn sync_audio_settings(menu: &MenuRuntime, settings: Option<ResMut<AudioSettings>>) {
    let Some(mut settings) = settings else {
        return;
    };
    let (options, _) = menu.settings_snapshot();
    for (name, category) in VOLUME_SLIDERS {
        if let Some(category) = category {
            let volume = options.value(name) as f32 / 100.0;
            if settings.volume(category) != volume {
                settings.set(category, volume);
            }
        }
    }
}

/// Samples session presence at the same end-of-frame boundary as the Discord driver.
pub(crate) fn publish_presence_observation(
    world: Res<ClientWorld>,
    session: Res<crate::session::SessionController>,
    mut observation: ResMut<client_state::PresenceObservation>,
) {
    observation.has_world = world.stream.is_some();
    observation.failed = world.fatal_error.is_some();
    observation.players = world
        .stream
        .as_ref()
        .map(|stream| u32::try_from(stream.authority().player_count()).unwrap_or(u32::MAX));
    if observation.target.as_ref() != session.presence_target() {
        observation.target = session.presence_target().cloned();
    }
}
