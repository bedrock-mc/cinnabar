use crate::runtime::network::resource_packs::PackApplication;
use bevy::prelude::IntoScheduleConfigs;
use client_runtime::network::NetworkHandle;
use std::path::PathBuf;

#[cfg(test)]
use bevy::prelude::{Entity, Mut, default};
use bevy::{
    prelude::{
        App, ButtonInput, KeyCode, MessageReader, MouseButton, NonSendMut, Query, Real, Res,
        ResMut, Resource, Time, Update, Window, With,
    },
    window::PrimaryWindow,
};
use server_experience::{policy::Choice, session::State, trust::Settings};

use client_extensions::server_experiences::consent::{can_disable, chrome};
use client_shell::ui_resources::{UiPresentationResource, UiRuntimeResource};
#[cfg(test)]
use client_ui::ui_runtime::{UiRuntime, presentation::UiPresentationRuntime};
use {client_shell::menu::MenuRuntime, client_state::ClientFrameSet};

#[derive(Resource)]
pub(super) struct ExperienceService {
    settings_path: PathBuf,
    cache_root: PathBuf,
    settings: Option<Settings>,
    generation: u64,
    attempted: bool,
    deployment: Option<(String, String)>,
    download: Option<server_experience::download::Download>,
    pub(super) live: Option<super::live::Live>,
    /// The session's items that client parts granted `items` read.
    items: client_ui::ui_runtime::session_data::SessionDataCache,
    /// This frame's keys pressed over a slot of the focused modal's inventory rows.
    hotbar: Option<(
        ui_screens::container_actions::InventoryCellHit,
        Vec<KeyCode>,
    )>,
}

impl ExperienceService {
    /// Drops every old helper and contribution before another signed deployment can run.
    fn select_deployment(&mut self, grant: &server_experience::negotiation::Grant) -> bool {
        let selected = (grant.session.clone(), grant.offer.digest.clone());
        let changed = self
            .deployment
            .as_ref()
            .is_some_and(|previous| previous != &selected);
        if changed {
            self.download = None;
            self.live = None;
            self.attempted = false;
        }
        self.deployment = Some(selected);
        changed
    }
}

/// Registers a silent controller; disk and network work wait for a valid marker.
pub(crate) fn configure(app: &mut App) {
    if !app.is_plugin_added::<render::ModRenderPlugin>() {
        app.add_plugins(render::ModRenderPlugin);
    }
    app.init_resource::<super::drawing::Environment>();
    app.add_systems(
        Update,
        super::drawing::present
            .after(crate::modding::DrawingPublication)
            .before(ClientFrameSet::UiPublication)
            .before(crate::environment::update_atmosphere_frame),
    );
    let menu = app.world().resource::<MenuRuntime>();
    let settings_path = menu.experience_settings_path();
    let cache_root = menu.experience_cache_dir();
    app.insert_resource(ExperienceService {
        settings_path,
        cache_root,
        settings: None,
        generation: 0,
        attempted: false,
        deployment: None,
        download: None,
        live: None,
        items: Default::default(),
        hotbar: None,
    })
    .add_systems(
        Update,
        (
            share_items,
            drive.before(client_state::ExperienceInputSet),
            modal_hotbar.before(client_state::ExperienceInputSet),
            present_media.after(client_state::ExperienceInputSet),
        )
            .chain()
            .before(ClientFrameSet::SemanticSample)
            .after(ClientFrameSet::RawInput)
            .after(crate::runtime::world::drain_committed_ui_before_authority),
    );
}

/// Hands the session's items to the client parts when one may read them.
fn share_items(
    mut service: ResMut<ExperienceService>,
    runtime: Res<UiRuntimeResource>,
    player: Res<client_state::PlayerRuntime>,
) {
    let ExperienceService {
        live: Some(live),
        items,
        ..
    } = &mut *service
    else {
        return;
    };
    if live.reads_items() {
        let session = client_ui::ui_runtime::session_data::session_data(items, &player, &runtime);
        live.set_session(session);
    }
}

/// A hotbar key pressed over a slot of the focused modal's inventory rows swaps that slot with
/// the hotbar slot, as in a container screen; over anything else it does nothing.
fn modal_hotbar(
    mut service: ResMut<ExperienceService>,
    menu: Option<Res<MenuRuntime>>,
    mut player: ResMut<client_state::PlayerRuntime>,
    mut runtime: ResMut<UiRuntimeResource>,
) {
    let Some((hit, keys)) = service.hotbar.take() else {
        return;
    };
    for key in keys {
        let Some(code) = client_shell::physical_input::keyboard_usage(key) else {
            continue;
        };
        let control = semantic_input::PhysicalControl::KeyboardUsage(code);
        for slot in client_shell::menu::settings_options::control_bindings::hotbar_control_slots(
            menu.as_deref(),
            control,
        ) {
            let result = client_ui::ui_runtime::interaction::dispatch_modal_hotbar(
                &mut player,
                &mut runtime,
                hit,
                slot,
            );
            if let Some(Err(error)) = result {
                bevy::log::debug!(?error, ?hit, slot, "modal hotbar swap refused");
            }
        }
    }
}

/// Handles trusted choices before ordinary UI input, so clicks cannot fall through.
#[allow(clippy::too_many_arguments, reason = "Bevy system parameters")]
fn drive(
    mut service: ResMut<ExperienceService>,
    mut runtime: ResMut<UiRuntimeResource>,
    mut presentation: ResMut<UiPresentationResource>,
    network: Res<NetworkHandle<PackApplication>>,
    world: Res<crate::runtime::world::ClientWorld>,
    fatal_error: Res<crate::runtime::shutdown::FatalError>,
    windows: Query<&Window, With<PrimaryWindow>>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut ownership: ResMut<client_state::ConsentInput>,
    time: Res<Time<Real>>,
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    mut buttons: MessageReader<bevy::input::mouse::MouseButtonInput>,
    mut keyboard: MessageReader<bevy::input::keyboard::KeyboardInput>,
    mut focus: Option<ResMut<client_state::CursorFocus>>,
    driven: Option<Res<client_state::DrivenInput>>,
) {
    let now_ms = u64::try_from(time.elapsed().as_millis()).unwrap_or(u64::MAX);
    let generation = runtime.session_id();
    let extension = &mut runtime.experiences;
    if service.generation != generation {
        service.generation = generation;
        service.deployment = None;
        service.attempted = false;
        service.download = None;
        service.live = None;
        let _ = presentation.mod_ui_mut().set_experience_chrome(None, false);
    }
    if fatal_error.0.is_some()
        || world.transfer_notice.is_some()
        || network.experience_failed()
        || presentation.mod_ui().experience_chrome_failed()
    {
        extension.session.disable();
    }
    if !extension.handled_marker
        && let Some(marker) = extension.marker.take()
    {
        extension.handled_marker = true;
        if let Some(audience) = &extension.audience {
            if service.settings.is_none() {
                service.settings = Some(Settings::load(&service.settings_path));
            }
            if let Some(settings) = service.settings.as_mut() {
                match extension.session.discover(
                    &marker,
                    audience,
                    settings,
                    client_ui::experience_session::unix_seconds(),
                    now_ms,
                ) {
                    Ok(true) => {
                        if std::env::var(server_experience::policy::DEVELOPER_ENV).as_deref()
                            != Ok("1")
                            || !std::env::current_exe().is_ok_and(|client| {
                                mod_host::helper::developer_runtime_available(&client)
                            })
                        {
                            extension.session.disable();
                            extension.session.notice = Some("Cinnabar: experience helper unavailable; using server fallback. F9: dismiss".into());
                            network.set_experience_enabled(false);
                        } else {
                            persist_trust(&mut service, &mut extension.session);
                        }
                    }
                    Ok(false) => {}
                    Err(error) => {
                        extension.session.disable();
                        bevy::log::warn!(%error, "server experience offer rejected");
                    }
                }
            }
        }
    }
    extension
        .session
        .tick(client_ui::experience_session::unix_seconds(), now_ms);
    let (text, wants_prompt) = chrome(&extension.session);
    if presentation
        .mod_ui_mut()
        .set_experience_chrome(text.as_deref(), wants_prompt)
        .is_err()
    {
        extension.session.disable();
    }
    let prompt = wants_prompt && presentation.mod_ui().experience_prompt_visible();
    let wheel_events: Vec<_> = wheel.read().map(|event| (event.y, event.unit)).collect();
    let wheel_delta: f64 = wheel_events
        .iter()
        .map(|(y, _)| -f64::from(*y) * 0.15)
        .sum();
    let window = windows.single().ok();
    let cursor = window
        .and_then(Window::cursor_position)
        .map(|point| point.to_array());
    if prompt {
        let pages = if keys.just_pressed(KeyCode::PageDown) || keys.just_pressed(KeyCode::ArrowDown)
        {
            1.0
        } else if keys.just_pressed(KeyCode::PageUp) || keys.just_pressed(KeyCode::ArrowUp) {
            -1.0
        } else {
            0.0
        };
        presentation
            .mod_ui_mut()
            .scroll_experience(pages + wheel_delta);
        presentation.mod_ui_mut().hover_experience(cursor);
    }
    let approval_ready = presentation.mod_ui().experience_approval_ready();
    let focused = driven.is_some()
        || (window.is_some_and(|window| window.focused)
            && focus.as_ref().is_none_or(|focus| focus.available()));
    let modal_focus = focused
        && !wants_prompt
        && service.live.is_some()
        && presentation.mod_ui().experience_modal_shown();
    if let Some(live) = service.live.as_mut() {
        live.set_modal_size(presentation.mod_ui().experience_modal_size());
        for (view, range) in presentation.mod_ui().experience_modal_scrolls() {
            if live.scroll_changed(&view, range) {
                presentation
                    .mod_ui_mut()
                    .accept_experience_modal_scroll(&view, range);
            }
        }
    }
    if let Some(live) = service.live.as_mut() {
        let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
        live.shift_changed(modal_focus && shift);
    }
    service.hotbar = None;
    if modal_focus {
        service.hotbar = cursor
            .and_then(|point| presentation.experience_modal_inventory_hit(point))
            .map(|hit| (hit, keys.get_just_pressed().copied().collect()));
        let notches = wheel_events
            .iter()
            .map(|(y, unit)| match unit {
                bevy::input::mouse::MouseScrollUnit::Line => -f64::from(*y),
                bevy::input::mouse::MouseScrollUnit::Pixel => -f64::from(*y) / 16.0,
            })
            .sum();
        presentation.mod_ui_mut().hover_experience_modal(cursor);
        presentation.mod_ui_mut().scroll_experience_modal(notches);
        let (mut pressed, mut released) = (false, false);
        let (mut secondary_pressed, mut secondary_released) = (false, false);
        for input in buttons.read() {
            match input.button {
                MouseButton::Left => {
                    pressed |= input.state.is_pressed();
                    released |= !input.state.is_pressed();
                }
                MouseButton::Right => {
                    secondary_pressed |= input.state.is_pressed();
                    secondary_released |= !input.state.is_pressed();
                }
                _ => {}
            }
        }
        let control = keys.any_pressed([
            KeyCode::ControlLeft,
            KeyCode::ControlRight,
            KeyCode::SuperLeft,
            KeyCode::SuperRight,
        ]);
        let typed: Vec<String> = keyboard
            .read()
            .filter(|input| input.state.is_pressed())
            .filter_map(|input| {
                client_ui::ui_runtime::forms::engine_input::typed_text(
                    client_shell::physical_input::keyboard_key(input.key_code),
                    input.text.as_deref(),
                    control,
                )
            })
            .collect();
        let escape = keys.just_pressed(KeyCode::Escape);
        let now = time.elapsed_secs_f64();
        let press = presentation
            .mod_ui_mut()
            .press_experience_modal(cursor, pressed, released);
        let secondary = presentation.mod_ui_mut().secondary_press_experience_modal(
            cursor,
            secondary_pressed,
            secondary_released,
        );
        let edits = presentation
            .mod_ui_mut()
            .edit_experience_modal(cursor, pressed, &typed, escape, now);
        let live = service.live.as_mut().expect("modal focus needs a runtime");
        for (control, text) in &edits.edits {
            live.text_changed(control, text);
        }
        if escape && !edits.escape_consumed {
            live.close_modal();
        } else {
            if let Some((id, index)) = press {
                live.press(&id, index);
            }
            if let Some((id, index)) = secondary {
                live.press_secondary(&id, index);
            }
        }
    } else {
        buttons.clear();
        keyboard.clear();
    }
    let choice =
        if focused && can_disable(&extension.session.state) && keys.just_pressed(KeyCode::F9) {
            Some(Choice::Disable)
        } else if focused && prompt {
            if approval_ready && keys.just_pressed(KeyCode::F6) {
                Some(Choice::Once)
            } else if approval_ready && keys.just_pressed(KeyCode::F7) {
                Some(Choice::Always)
            } else if keys.just_pressed(KeyCode::F8) {
                Some(Choice::Never)
            } else if keys.just_pressed(KeyCode::Escape) {
                Some(Choice::Cancel)
            } else if mouse.just_pressed(MouseButton::Left) {
                cursor.and_then(|point| presentation.mod_ui().experience_choice(point))
            } else {
                None
            }
        } else {
            None
        };
    ownership.0 = wants_prompt || choice.is_some() || modal_focus;
    if let Some(choice) = choice {
        let result = service
            .settings
            .as_mut()
            .map(|settings| extension.session.choose(choice, settings, now_ms));
        match result {
            Some(Ok(true)) => persist_trust(&mut service, &mut extension.session),
            Some(Err(error)) => {
                extension.session.disable();
                bevy::log::warn!(%error, "server experience choice rejected");
            }
            _ => {}
        }
        if wants_prompt
            && !matches!(extension.session.state, State::Offered(_))
            && let Some(focus) = focus.as_deref_mut()
        {
            focus.authorize_screen_return();
        }
    }
    network.set_experience_enabled(
        matches!(extension.session.state, State::Awaiting(_))
            || matches!(extension.session.state, State::Granted(_))
                && (service.download.is_some() || service.live.is_some()),
    );
    if let Some(bytes) = extension.session.take_outbound() {
        let sent = protocol::experience_packet(bytes)
            .is_some_and(|packet| network.send_form_packet(generation, packet).is_ok());
        if !sent {
            extension.session.disable();
        }
    }
    if let Err(error) = advance_runtime(
        &mut service,
        extension,
        &network,
        generation,
        now_ms,
        world
            .stream
            .as_ref()
            .map(|stream| {
                (
                    stream.authority().actor_session_id(),
                    stream.authority().current_dimension(),
                )
            })
            .unwrap_or_default(),
    ) {
        extension.session.disable();
        extension.active = false;
        service.download = None;
        service.live = None;
        bevy::log::warn!(%error, "server experience runtime disabled");
    }
    let (text, prompt) = chrome(&extension.session);
    if let Err(error) = presentation
        .mod_ui_mut()
        .set_experience_chrome(text.as_deref(), prompt)
    {
        extension.session.disable();
        bevy::log::warn!(%error, "server experience trusted UI unavailable");
    }
    let labels = service.live.as_ref().map(super::live::Live::labels);
    presentation
        .mod_ui_mut()
        .set_experience_labels(labels.as_deref().unwrap_or_default());
    presentation.set_experience_modal(service.live.as_ref().and_then(super::live::Live::modal));
    if let Some(error) = presentation.mod_ui().experience_modal_failure() {
        bevy::log::warn!(%error, "client part screen rejected; server code disabled");
        extension.session.disable();
    }
    if matches!(extension.session.state, State::Disabled) {
        network.set_experience_enabled(false);
        service.download = None;
        service.live = None;
        extension.active = false;
    }
}

/// Advances media players on the monotonic clock and publishes their screens and audio.
fn present_media(
    mut service: ResMut<ExperienceService>,
    time: Res<Time<Real>>,
    scene: Option<ResMut<render_scene::MediaScreenScene>>,
    device: Option<NonSendMut<client_audio::named_audio::AudioDevice>>,
    audio_settings: Option<Res<client_audio::audio::settings::AudioSettings>>,
) {
    let local_us = u64::try_from(time.elapsed().as_micros()).unwrap_or(u64::MAX);
    let muted = service
        .settings
        .as_ref()
        .is_some_and(|settings| settings.media_muted);
    let mut scene = scene;
    if let Some(live) = &mut service.live {
        // Consent already covers playback here; the autoplay preference awaits a settings UI.
        live.media_mut().service(
            client_ui::experience_session::unix_seconds(),
            local_us,
            true,
        );
        let defaults = client_audio::audio::settings::AudioSettings::default();
        let settings = audio_settings.as_deref().unwrap_or(&defaults);
        live.media_mut().pump_audio(
            device.map(NonSendMut::into_inner),
            settings,
            muted,
            local_us,
        );
        let budget = live.gpu_budget_bytes();
        // Writes only on change, so an unchanged scene neither allocates nor re-extracts.
        if let Some(screens) = live.changed_screens()
            && let Some(scene) = scene.as_mut()
            && (!screens.is_empty() || !scene.screens.is_empty())
        {
            scene.screens.clear();
            scene.screens.extend_from_slice(screens);
            scene.gpu_budget_bytes = budget;
        }
    } else if let Some(scene) = scene.as_mut()
        && !scene.screens.is_empty()
    {
        scene.screens.clear();
    }
}

/// Rolls back unsaved in-memory approval as well as revoking its pending handshake.
fn persist_trust(
    service: &mut ExperienceService,
    session: &mut server_experience::session::Session,
) {
    if let Some(settings) = &service.settings
        && let Err(error) = settings.save(&service.settings_path)
    {
        session.disable();
        service.settings = Some(Settings::load(&service.settings_path));
        bevy::log::warn!(%error, "server experience trust could not be saved");
    }
}

/// Starts work only for a live signed grant and discards it on every revocation.
fn advance_runtime(
    service: &mut ExperienceService,
    extension: &mut client_ui::experience_session::ExperienceSession,
    network: &NetworkHandle<PackApplication>,
    generation: u64,
    now_ms: u64,
    drawing_fence: (u64, i32),
) -> anyhow::Result<()> {
    let State::Granted(grant) = &extension.session.state else {
        service.download = None;
        service.live = None;
        extension.active = false;
        while extension.pop().is_some() {}
        return Ok(());
    };
    let deployment_changed = service.select_deployment(grant);
    if !service.attempted {
        service.attempted = true;
        if std::env::var(server_experience::policy::DEVELOPER_ENV).as_deref() != Ok("1") {
            extension.session.notice = Some(
                "Cinnabar: restricted runtime unavailable; using server fallback. F9: dismiss"
                    .into(),
            );
            network.set_experience_enabled(false);
            return Ok(());
        }
        network.set_experience_enabled(true);
        service.download = Some(server_experience::download::Download::start(
            grant.clone(),
            service.cache_root.clone(),
        )?);
        extension.session.notice =
            Some("Cinnabar: downloading approved experience. F9: cancel".into());
    }
    if let Some(result) = service
        .download
        .as_ref()
        .and_then(|download| download.poll())
    {
        service.download = None;
        let client = std::env::current_exe()?;
        service.live = Some(super::live::Live::start(
            grant.clone(),
            result?,
            extension.epoch,
            now_ms,
            &mod_host::helper::developer_executable(&client),
            &mod_host::helper::media_executable(&client),
        )?);
        extension.active = true;
    }
    if deployment_changed {
        while extension.pop().is_some() {}
    }
    if let Some(live) = &mut service.live {
        live.bind_drawing(drawing_fence.0, drawing_fence.1);
        while let Some((received_ms, bytes)) = extension.pop() {
            live.receive(&bytes, received_ms)?;
        }
        for bytes in live.poll(extension.epoch, now_ms)? {
            let packet = protocol::experience_packet(bytes)
                .ok_or_else(|| anyhow::anyhow!("outbound envelope too large"))?;
            anyhow::ensure!(
                network.send_form_packet(generation, packet).is_ok(),
                "extension send unavailable"
            );
        }
        extension.session.notice = Some(live.text());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
