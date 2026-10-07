use std::path::PathBuf;

use bevy::{prelude::*, window::PrimaryWindow};
use server_experience::{
    session::State,
    trust::{Choice, Settings},
};

use crate::{app::ClientFrameSet, menu::MenuRuntime, runtime::network::NetworkHandle};
use client_ui::ui_runtime::{UiRuntime, presentation::UiPresentationRuntime};

#[derive(Resource)]
pub(super) struct ExperienceService {
    settings_path: PathBuf,
    cache_root: PathBuf,
    settings: Option<Settings>,
    generation: u64,
    attempted: bool,
    download: Option<server_experience::download::Download>,
    live: Option<super::live::Live>,
}

/// Registers a silent controller; disk and network work wait for a valid marker.
pub(crate) fn configure(app: &mut App) {
    let menu = app.world().resource::<MenuRuntime>();
    let settings_path = menu.experience_settings_path();
    let cache_root = menu.experience_cache_dir();
    app.init_resource::<super::input::ConsentInput>();
    app.insert_resource(ExperienceService {
        settings_path,
        cache_root,
        settings: None,
        generation: 0,
        attempted: false,
        download: None,
        live: None,
    })
    .add_systems(
        Update,
        (drive, super::input::consume, present_media)
            .chain()
            .before(ClientFrameSet::SemanticSample)
            .after(ClientFrameSet::RawInput)
            .after(crate::runtime::world::drain_committed_ui_before_authority),
    );
}

/// Handles trusted choices before ordinary UI input, so clicks cannot fall through.
#[allow(clippy::too_many_arguments, reason = "Bevy system parameters")]
fn drive(
    mut service: ResMut<ExperienceService>,
    mut runtime: ResMut<UiRuntime>,
    mut presentation: ResMut<UiPresentationRuntime>,
    network: Res<NetworkHandle>,
    world: Res<crate::runtime::world::ClientWorld>,
    windows: Query<&Window, With<PrimaryWindow>>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut ownership: ResMut<super::input::ConsentInput>,
    time: Res<Time<Real>>,
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    mut buttons: MessageReader<bevy::input::mouse::MouseButtonInput>,
    mut keyboard: MessageReader<bevy::input::keyboard::KeyboardInput>,
    mut focus: Option<ResMut<client_presentation::camera::CursorFocus>>,
    driven: Option<Res<crate::camera::DrivenInput>>,
) {
    let now_ms = u64::try_from(time.elapsed().as_millis()).unwrap_or(u64::MAX);
    let generation = runtime.session_id();
    let extension = &mut runtime.experiences;
    if service.generation != generation {
        service.generation = generation;
        service.attempted = false;
        service.download = None;
        service.live = None;
        let _ = presentation.set_experience_chrome(None, false);
    }
    if world.fatal_error.is_some()
        || world.transfer_notice.is_some()
        || network.experience_failed()
        || presentation.experience_chrome_failed()
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
                    super::unix_seconds(),
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
    extension.session.tick(super::unix_seconds(), now_ms);
    let (text, wants_prompt) = chrome(&extension.session);
    if presentation
        .set_experience_chrome(text.as_deref(), wants_prompt)
        .is_err()
    {
        extension.session.disable();
    }
    let prompt = wants_prompt && presentation.experience_prompt_visible();
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
        presentation.scroll_experience(pages + wheel_delta);
        presentation.hover_experience(cursor);
    }
    let approval_ready = presentation.experience_approval_ready();
    let focused = driven.is_some()
        || (window.is_some_and(|window| window.focused)
            && focus.as_ref().is_none_or(|focus| focus.available()));
    let modal_focus =
        focused && !wants_prompt && service.live.is_some() && presentation.experience_modal_shown();
    if let Some(live) = service.live.as_mut() {
        live.set_modal_size(presentation.experience_modal_size());
    }
    if modal_focus {
        let notches = wheel_events
            .iter()
            .map(|(y, unit)| match unit {
                bevy::input::mouse::MouseScrollUnit::Line => -f64::from(*y),
                bevy::input::mouse::MouseScrollUnit::Pixel => -f64::from(*y) / 16.0,
            })
            .sum();
        presentation.hover_experience_modal(cursor);
        presentation.scroll_experience_modal(notches);
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
                crate::ui_runtime::typed_text(input.key_code, input.text.as_deref(), control)
            })
            .collect();
        let escape = keys.just_pressed(KeyCode::Escape);
        let now = time.elapsed_secs_f64();
        let press = presentation.press_experience_modal(cursor, pressed, released);
        let secondary = presentation.secondary_press_experience_modal(
            cursor,
            secondary_pressed,
            secondary_released,
        );
        let edits = presentation.edit_experience_modal(cursor, pressed, &typed, escape, now);
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
                cursor.and_then(|point| presentation.experience_choice(point))
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
    if let Err(error) = advance_runtime(&mut service, extension, &network, generation, now_ms) {
        extension.session.disable();
        extension.active = false;
        service.download = None;
        service.live = None;
        bevy::log::warn!(%error, "server experience runtime disabled");
    }
    let (text, prompt) = chrome(&extension.session);
    if let Err(error) = presentation.set_experience_chrome(text.as_deref(), prompt) {
        extension.session.disable();
        bevy::log::warn!(%error, "server experience trusted UI unavailable");
    }
    let labels = service.live.as_ref().map(super::live::Live::labels);
    presentation.set_experience_labels(labels.as_deref().unwrap_or_default());
    presentation.set_experience_modal(service.live.as_ref().and_then(super::live::Live::modal));
    if let Some(error) = presentation.experience_modal_failure() {
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
    scene: Option<ResMut<render::MediaScreenScene>>,
    device: Option<NonSendMut<client_presentation::named_audio::AudioDevice>>,
    audio_settings: Option<Res<client_presentation::audio::settings::AudioSettings>>,
) {
    let local_us = u64::try_from(time.elapsed().as_micros()).unwrap_or(u64::MAX);
    let muted = service
        .settings
        .as_ref()
        .is_some_and(|settings| settings.media_muted);
    let mut scene = scene;
    if let Some(live) = &mut service.live {
        // Consent already covers playback here; the autoplay preference awaits a settings UI.
        live.media_mut()
            .service(super::unix_seconds(), local_us, true);
        let defaults = client_presentation::audio::settings::AudioSettings::default();
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
    extension: &mut super::ExperienceSession,
    network: &NetworkHandle,
    generation: u64,
    now_ms: u64,
) -> anyhow::Result<()> {
    let State::Granted(grant) = &extension.session.state else {
        service.download = None;
        service.live = None;
        extension.active = false;
        while extension.pop().is_some() {}
        return Ok(());
    };
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
    if let Some(live) = &mut service.live {
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

/// Builds plain trusted text; pack data can only fill labeled values. Only an unanswered
/// offer prompts, over whatever is on screen: the join holds its loading screen meanwhile.
fn chrome(session: &server_experience::session::Session) -> (Option<String>, bool) {
    let text = match &session.state {
        State::Inert | State::Disabled => return (None, false),
        State::Offered(offer) => {
            let packages = offer
                .offer
                .packages
                .iter()
                .map(|package| {
                    format!(
                        "{} ({} bytes)\nPublisher: {}",
                        package.id, package.bytes, package.publisher_key,
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            let identity = if session.key_changed {
                "Server key changed: new approval required."
            } else {
                "First-use key pinning does not verify the operator's identity."
            };
            format!(
                concat!(
                    "Server: {}\nKey: {}\n{}\n{}\n",
                    "Permissions: {:?}\nMemory limit: {} bytes; GPU limit: {} bytes\n",
                    "Media/download hosts: {}\nThese hosts see your IP address.\n",
                    "Fallback: {}\nServer code is untrusted. F9 disables it immediately.",
                ),
                offer.offer.audience,
                offer.offer.server_key,
                identity,
                packages,
                offer.offer.scope.permissions,
                offer.offer.scope.memory_bytes,
                offer.offer.scope.gpu_bytes,
                offer
                    .offer
                    .scope
                    .origins
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("\n"),
                offer.offer.fallback,
            )
        }
        State::Awaiting(_) => "Cinnabar: verifying server experience. F9: disable".into(),
        State::Granted(_) => session.notice.clone().unwrap_or_else(|| {
            "Cinnabar: experience approved; runtime unavailable. F9: disable".into()
        }),
    };
    (Some(text), matches!(session.state, State::Offered(_)))
}

/// Leaves all ordinary input untouched when there is no offered experience.
fn can_disable(state: &State) -> bool {
    matches!(
        state,
        State::Offered(_) | State::Awaiting(_) | State::Granted(_)
    )
}

#[cfg(test)]
mod status_tests {
    use super::*;

    #[test]
    fn vanilla_session_does_not_claim_disable_key_or_draw_chrome() {
        let session = server_experience::session::Session::default();
        assert!(!can_disable(&session.state));
        assert_eq!(chrome(&session), (None, false));
        assert!(!can_disable(&State::Disabled));
    }
}

#[cfg(test)]
mod tests;
