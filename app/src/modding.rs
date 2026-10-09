//! Opt-in component spike. The default client registers no extension runtime.

#[cfg(feature = "local-mods")]
pub(crate) mod font;

use bevy::prelude::*;
#[cfg(feature = "local-mods")]
use {
    crate::{app::ClientFrameSet, environment::VisualTimeOverride, menu::MenuRuntime},
    bevy::window::{CursorOptions, PrimaryWindow},
    client_ui::ui_runtime::{UiRuntime, presentation::UiPresentationRuntime},
    mod_host::{ModGrants, ModHost},
    std::{
        path::Path,
        time::{Duration, Instant},
    },
};

const COMPONENT_ENV: &str = "CINNABAR_MOD_COMPONENT";
#[cfg(feature = "local-mods")]
const PLAYERS_ENV: &str = "CINNABAR_MOD_PLAYERS";
#[cfg(feature = "local-mods")]
const PLAYER_STATE_ENV: &str = "CINNABAR_MOD_PLAYER_STATE";
#[cfg(feature = "local-mods")]
const ITEM_USE_ENV: &str = "CINNABAR_MOD_ITEM_USE";
#[cfg(feature = "local-mods")]
const CAMERA_ENV: &str = "CINNABAR_MOD_CAMERA";
#[cfg(feature = "local-mods")]
const CONTROLS_ENV: &str = "CINNABAR_MOD_CONTROLS";
#[cfg(feature = "local-mods")]
const HUD_ENV: &str = "CINNABAR_MOD_HUD";
#[cfg(feature = "local-mods")]
const MOVEMENT_ENV: &str = "CINNABAR_MOD_MOVEMENT";
#[cfg(feature = "local-mods")]
const INTERACTION_ENV: &str = "CINNABAR_MOD_INTERACTION";
#[cfg(feature = "local-mods")]
const SETTINGS_ENV: &str = "CINNABAR_MOD_SETTINGS";
#[cfg(feature = "local-mods")]
const RENDER_ENV: &str = "CINNABAR_MOD_RENDER";
#[cfg(feature = "local-mods")]
const BLOCK_HIGHLIGHTS_ENV: &str = "CINNABAR_MOD_BLOCK_HIGHLIGHTS";
#[cfg(feature = "local-mods")]
const FULLBRIGHT_ENV: &str = "CINNABAR_MOD_FULLBRIGHT";
#[cfg(feature = "local-mods")]
const RENDER_DEPTH_ENV: &str = "CINNABAR_MOD_RENDER_DEPTH";
#[cfg(feature = "local-mods")]
const ENTITIES_ENV: &str = "CINNABAR_MOD_ENTITIES";
/// Comma-separated command names the selected component may request.
#[cfg(feature = "local-mods")]
const COMMANDS_ENV: &str = "CINNABAR_MOD_COMMANDS";
#[cfg(feature = "local-mods")]
const PACKET_DELAY_ENV: &str = "CINNABAR_MOD_PACKET_DELAY";
#[cfg(feature = "local-mods")]
const PACKET_DELAY_VISUAL_ENV: &str = "CINNABAR_MOD_PACKET_DELAY_VISUAL";
#[cfg(feature = "local-mods")]
const DEMO_KEY: KeyCode = KeyCode::F8;
#[cfg(feature = "local-mods")]
const RELOAD_INTERVAL: Duration = Duration::from_millis(500);

#[cfg(feature = "local-mods")]
mod hud_editor;
#[cfg(feature = "local-mods")]
mod multi;
#[cfg(feature = "local-mods")]
mod registration;

/// Every loaded mod's presentation cues committed this frame, in load order.
#[cfg(feature = "local-mods")]
#[derive(Resource, Default)]
pub(crate) struct ModCueFeed(pub Vec<mod_host::ModCue>);

#[cfg(feature = "local-mods")]
#[derive(Resource)]
struct ModRuntime {
    host: ModHost,
    companions: Vec<multi::Companion>,
    /// Merged label of several mods, rebuilt only when one of their labels changes.
    label: Option<String>,
    label_inputs: Vec<String>,
    label_rebuilds: u64,
    /// Per-mod render generations behind the last merged scene.
    render_sources: Vec<u64>,
    render_merge: mod_host::mod_render::RenderMerge,
    last_reload: Instant,
    controls: mod_host::ControlFrame,
    reload_on_main: bool,
    registration_identity: Option<[u8; 32]>,
    registration_request: Option<(u64, String)>,
    suspended: bool,
    hud_editor_owner: Option<hud_editor::Owner>,
}

/// Bounded personal-extension diagnostics, only through the developer control endpoint.
#[cfg(all(feature = "developer-control", feature = "local-mods"))]
pub(crate) fn developer_state(world: &World) -> Option<serde_json::Value> {
    let runtime = world.get_resource::<ModRuntime>()?;
    let hosts: Vec<_> = (0..runtime.host_count())
        .map(|index| {
            let host = runtime.host(index);
            serde_json::json!({
                "index": index,
                "active": host.is_active(),
                "label": host.label(),
                "panel_open": host.panel_open(),
                "reserved_keys": host.reserved_keys(),
                "movement_granted": host.grants().movement,
                "panel": host.panel(),
            })
        })
        .collect();
    Some(serde_json::json!({ "suspended": runtime.suspended, "hosts": hosts }))
}

/// Installs the developer extension only when its component path is explicit.
pub(crate) fn configure_from_environment(app: &mut App) {
    let path = std::env::var_os(COMPONENT_ENV);
    #[cfg(feature = "local-mods")]
    if let Some(set) = std::env::var_os(multi::SET_ENV) {
        match multi::read_set(Path::new(&set)) {
            Ok(mods) => configure_set(app, mods),
            Err(error) => eprintln!("Local mod set disabled: {error}"),
        }
    } else if let Some(path) = path.as_deref() {
        configure(app, Some(Path::new(path)));
    } else {
        match launcher::install_layout::InstallLayout::discover()
            .map_err(|error| error.to_string())
            .and_then(|layout| registration::Watcher::start(layout.user_config_root))
        {
            Ok(watcher) => {
                app.insert_resource(watcher);
                configure_systems(app, true);
            }
            Err(error) => eprintln!("Local extension watcher unavailable: {error}"),
        }
    }
    #[cfg(not(feature = "local-mods"))]
    if path.is_some() {
        let _ = app;
        eprintln!("{COMPONENT_ENV} ignored: build bedrock-client with --features local-mods");
    }
}

/// Loads one optional component without changing the vanilla schedule on absence.
#[cfg(feature = "local-mods")]
fn configure(app: &mut App, path: Option<&Path>) {
    let grants = ModGrants {
        environment: true,
        players: std::env::var(PLAYERS_ENV).is_ok_and(|value| value == "1"),
        player_state: std::env::var(PLAYER_STATE_ENV).is_ok_and(|value| value == "1"),
        item_use: std::env::var(ITEM_USE_ENV).is_ok_and(|value| value == "1"),
        camera: std::env::var(CAMERA_ENV).is_ok_and(|value| value == "1"),
        controls: std::env::var(CONTROLS_ENV).is_ok_and(|value| value == "1"),
        hud: std::env::var(HUD_ENV).is_ok_and(|value| value == "1"),
        movement: std::env::var(MOVEMENT_ENV).is_ok_and(|value| value == "1"),
        interaction: std::env::var(INTERACTION_ENV).is_ok_and(|value| value == "1"),
        settings: std::env::var(SETTINGS_ENV).is_ok_and(|value| value == "1"),
        render: std::env::var(RENDER_ENV).is_ok_and(|value| value == "1"),
        block_highlights: std::env::var(BLOCK_HIGHLIGHTS_ENV).is_ok_and(|value| value == "1"),
        fullbright: std::env::var(FULLBRIGHT_ENV).is_ok_and(|value| value == "1"),
        render_depth: std::env::var(RENDER_DEPTH_ENV).is_ok_and(|value| value == "1"),
        entities: std::env::var(ENTITIES_ENV).is_ok_and(|value| value == "1"),
        commands: std::env::var(COMMANDS_ENV)
            .map(|names| {
                names
                    .split(',')
                    .filter(|name| !name.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
        packet_delay: std::env::var(PACKET_DELAY_ENV).is_ok_and(|value| value == "1")
            || std::env::var(PACKET_DELAY_VISUAL_ENV).is_ok_and(|value| value == "1"),
    };
    configure_with_grants(app, path, grants);
}

/// Grants are explicit and apply only to the selected personal component.
#[cfg(feature = "local-mods")]
fn configure_with_grants(app: &mut App, path: Option<&Path>, grants: ModGrants) {
    if let Some(path) = path {
        configure_set(app, vec![(path.to_owned(), grants)]);
    }
}

/// Loads components in order, each with its own grants; one failing to load leaves the rest.
#[cfg(feature = "local-mods")]
fn configure_set(app: &mut App, mods: Vec<(std::path::PathBuf, ModGrants)>) {
    let mut hosts = mods.into_iter().filter_map(|(path, grants)| {
        ModHost::load_with_grants(&path, grants)
            .inspect_err(|error| {
                eprintln!("Cinnabar extension {} disabled: {error:#}", path.display());
            })
            .ok()
    });
    let Some(host) = hosts.next() else { return };
    let companions: Vec<_> = hosts.map(|host| multi::Companion { host }).collect();
    let controls = host.grants().controls
        || companions
            .iter()
            .any(|companion| companion.host.grants().controls);
    app.insert_resource(VisualTimeOverride(host.time_override()))
        .insert_resource(ModRuntime {
            host,
            companions,
            label: None,
            label_inputs: Vec::new(),
            label_rebuilds: 0,
            render_sources: Vec::new(),
            render_merge: Default::default(),
            last_reload: Instant::now(),
            controls: mod_host::empty_controls(),
            reload_on_main: true,
            registration_identity: None,
            registration_request: None,
            suspended: false,
            hud_editor_owner: None,
        })
        .init_resource::<interaction::ModInteraction>();
    if controls && let Some(path) = std::env::var_os(font::FONT_ENV) {
        match font::load(Path::new(&path)).and_then(|font| {
            app.world_mut()
                .get_resource_mut::<UiPresentationRuntime>()
                .ok_or_else(|| "presentation is unavailable".to_owned())?
                .set_mod_panel_font(Some(std::sync::Arc::new(font)))
                .map_err(|error| error.to_string())
        }) {
            Ok(()) => {}
            Err(error) => eprintln!("Optional personal-panel font unavailable: {error}"),
        }
    }
    configure_systems(app, false);
}

#[cfg(feature = "local-mods")]
fn configure_systems(app: &mut App, watching: bool) {
    ghost::configure(app);
    block_highlights::configure(app);
    fullbright::configure(app);
    app.init_resource::<crate::item_use::ModItemUsePolicy>()
        .init_resource::<ModCueFeed>()
        .add_plugins(::render::ModRenderPlugin)
        .add_systems(Update, render::grant_depth_sampling);
    if watching {
        app.add_systems(
            Update,
            (
                registration::install_pending,
                ApplyDeferred,
                input::prepare_mod_input,
            )
                .chain()
                .before(ClientFrameSet::RawInput),
        );
    } else {
        app.add_systems(
            Update,
            input::prepare_mod_input.before(ClientFrameSet::RawInput),
        );
    }
    app.add_systems(
        Update,
        drive_mod
            .in_set(crate::camera::ModCameraInputSet)
            .after(ClientFrameSet::SemanticFinalize)
            .before(ClientFrameSet::UiPublication)
            .before(crate::environment::update_atmosphere_frame),
    );
    app.init_resource::<packet_delay::RealPositionSnapshot>()
        .add_systems(Update, packet_delay::publish_packet_delay.after(drive_mod));
}

/// Runs the bounded guest and publishes only its validated presentation output.
/// Where a mod's network, camera and cue output lands.
#[cfg(feature = "local-mods")]
type ModOutputs<'w> = (
    Option<Res<'w, crate::runtime::network::NetworkHandle>>,
    Option<ResMut<'w, crate::camera::CameraSettingsAuthority>>,
    Option<ResMut<'w, ModCueFeed>>,
    Option<ResMut<'w, crate::item_use::ModItemUsePolicy>>,
);

#[allow(
    clippy::too_many_arguments,
    reason = "Player authority is borrowed separately from UI state."
)]
#[cfg(feature = "local-mods")]
fn drive_mod(
    player_runtime: bevy::prelude::Res<crate::player_runtime::PlayerRuntime>,
    extension: Option<ResMut<ModRuntime>>,
    keys: Res<ButtonInput<KeyCode>>,
    focus: Option<Res<client_presentation::camera::CursorFocus>>,
    driven: Option<Res<crate::camera::DrivenInput>>,
    windows: Query<(&Window, Option<&CursorOptions>), With<PrimaryWindow>>,
    ui: Res<UiRuntime>,
    menu: Option<Res<MenuRuntime>>,
    mut presentation: ResMut<UiPresentationRuntime>,
    time_override: Option<ResMut<VisualTimeOverride>>,
    mut gameplay: gameplay::GameplayContext,
    interaction: Option<ResMut<interaction::ModInteraction>>,
    watcher: Option<Res<registration::Watcher>>,
    render_scene: Option<ResMut<::render::ModRenderScene>>,
    mut outputs: ModOutputs,
) {
    let focused = windows.single().is_ok_and(|(window, _)| window.focused);
    let absorbed = crate::screen_policy::absorbs_input(
        &player_runtime,
        Some(&ui),
        menu.as_deref(),
        Some(&presentation),
    );
    let captured = windows.single().is_ok_and(|(window, cursor)| {
        cursor.is_some_and(|cursor| {
            crate::camera::mouse_input_active(window, cursor, focus.as_deref(), driven.is_some())
        })
    });
    let movement_allowed = extension.as_ref().is_some_and(|runtime| {
        !runtime.suspended
            && (0..runtime.host_count()).any(|index| {
                runtime.host(index).is_active() && runtime.host(index).grants().movement
            })
    });
    gameplay.synchronize_jump_scope(movement_allowed && captured && !absorbed);
    let (Some(mut extension), Some(mut time_override), Some(mut interaction)) =
        (extension, time_override, interaction)
    else {
        return;
    };
    if extension.suspended {
        return;
    }
    let mut reloaded = false;
    if extension.last_reload.elapsed() >= RELOAD_INTERVAL {
        extension.last_reload = Instant::now();
        let first = usize::from(!extension.reload_on_main);
        for index in first..extension.host_count() {
            match extension.host_mut(index).reload_if_changed() {
                // A new instance never sees cues from before it existed.
                Ok(true) => {
                    if extension
                        .hud_editor_owner
                        .is_some_and(|owner| owner.host == index)
                    {
                        hud_editor::cancel(&mut extension, &mut presentation, false);
                    }
                    reloaded = true;
                    if let Some(cues) = outputs.2.as_mut() {
                        cues.0.clear();
                    }
                }
                Ok(false) => {}
                Err(error) => eprintln!("Cinnabar extension reload rejected: {error:#}"),
            }
        }
    }
    if reloaded {
        gameplay.synchronize_jump_scope(false);
        gameplay.synchronize_jump_scope(movement_allowed && captured && !absorbed);
    }
    let pressed = keybind_allowed(focused, absorbed) && keys.just_pressed(DEMO_KEY);
    let controls = std::mem::replace(&mut extension.controls, mod_host::empty_controls());
    let (network, camera, cues, item_use) = outputs;
    let previous_cues = cues.as_ref().map_or_else(Vec::new, |feed| feed.0.clone());
    let registration = extension.registration_request.clone();
    let player_state = gameplay.player_state(
        (0..extension.host_count()).any(|index| extension.host(index).grants().player_state),
        &player_runtime,
        &ui,
    );
    let mut callback_failed = false;
    let merged = multi::run_frame(
        &mut extension,
        multi::FrameInput {
            pressed,
            controls: &controls,
            previous_cues: &previous_cues,
            player_state: player_state.as_ref(),
        },
        |grants| {
            let snapshot = gameplay.snapshot(captured && !absorbed, grants);
            let mobs = gameplay.mobs(snapshot.as_ref(), grants);
            let movement = gameplay.movement_snapshot(captured && !absorbed, grants);
            (snapshot, mobs, movement)
        },
        |index, error| {
            callback_failed = true;
            if index == 0
                && let Some((generation, request_id)) = &registration
                && let Some(watcher) = watcher.as_ref()
            {
                watcher.quarantine(*generation, request_id.clone(), error.clone());
            }
            eprintln!("Cinnabar extension callback failed: {error}");
        },
    );
    render::publish(render_scene, &mut extension);
    if let Some(watcher) = watcher.as_ref() {
        watcher.remember_settings(Some(&extension.host));
    }
    if callback_failed || merged.jump_cancel {
        gameplay.synchronize_jump_scope(false);
    }
    let movement_allowed = (0..extension.host_count())
        .any(|index| extension.host(index).is_active() && extension.host(index).grants().movement);
    gameplay.synchronize_jump_scope(movement_allowed && captured && !absorbed);
    gameplay.pulse_jump(merged.jump_pulse && !merged.jump_cancel);
    interaction.attack_reach = merged.attack_reach;
    interaction.attack_pulse = merged.attack_pulse && gameplay.pulse_attack();
    if let Some(delta) = merged.delta {
        gameplay.apply(delta);
    }
    send_commands(network.as_deref(), ui.session_id(), merged.commands);
    if let Some(mut cues) = cues {
        cues.0 = merged.cues;
    }
    if let Some(mut camera) = camera {
        camera.set_rig(merged.rig.map(camera_rig));
        camera.set_preserve_teleport_rotation(merged.preserve_teleport_rotation);
        let [fov_scale, look_scale] = merged.view_scale.unwrap_or([1.0, 1.0]);
        camera.set_view_scale(fov_scale, look_scale);
    }
    if let Some(mut policy) = item_use {
        policy.scope = merged.item_use_delay_fix;
    }
    time_override.0 = merged.time_override;
    if let Err(error) = presentation.set_mod_label(extension.merged_label()) {
        eprintln!("Cinnabar extension HUD rejected: {error}");
    }
    let hud_owner = extension.hud_owner();
    let hud = hud_owner.and_then(|owner| extension.host(owner).hud());
    if let Err(error) = presentation.set_mod_hud(hud) {
        eprintln!("Cinnabar extension HUD cards rejected: {error}");
    }
    let crosshair_owner = extension.crosshair_owner();
    let crosshair = crosshair_owner.and_then(|owner| extension.host(owner).crosshair());
    if let Err(error) = presentation.set_mod_crosshair(crosshair) {
        eprintln!("Cinnabar extension crosshair rejected: {error}");
    }
    let owner = extension.panel_owner();
    if let Err(error) = presentation.set_mod_panel(extension.host(owner).panel()) {
        eprintln!("Cinnabar extension panel rejected: {error}");
        extension.host_mut(owner).set_panel_open(false);
    }
    presentation.set_mod_panel_open(extension.host(owner).panel_open());
    let editor_session = gameplay.hud_editor_session(&ui);
    hud_editor::publish(
        &mut extension,
        &mut presentation,
        editor_session,
        focused || driven.is_some(),
    );
}

/// Granted commands travel the session-fenced UI packet lane as vanilla command requests.
#[cfg(feature = "local-mods")]
fn send_commands(
    network: Option<&crate::runtime::network::NetworkHandle>,
    session: u64,
    commands: Vec<String>,
) {
    let Some(network) = network else { return };
    for command in commands {
        if network
            .send_form_packet(session, protocol::command_request_packet(&command))
            .is_err()
        {
            eprintln!("Cinnabar extension command dropped: the session is not accepting packets");
            return;
        }
    }
}

#[cfg(feature = "local-mods")]
fn camera_rig(rig: mod_host::GameplayCameraRig) -> crate::camera::CameraRig {
    crate::camera::CameraRig {
        offset: Vec3::new(rig.offset.x, rig.offset.y, rig.offset.z),
        roll_radians: rig.roll,
        fov_delta_degrees: rig.fov_delta,
    }
}

/// A mod keybind is unavailable while another UI or an unfocused window owns input.
#[cfg(feature = "local-mods")]
fn keybind_allowed(window_focused: bool, input_absorbed: bool) -> bool {
    window_focused && !input_absorbed
}

#[cfg(all(test, feature = "local-mods"))]
mod tests {
    use super::*;

    #[test]
    fn no_mod_path_registers_no_runtime_or_systems() {
        let mut app = App::new();
        configure(&mut app, None);
        assert!(!app.world().contains_resource::<ModRuntime>());
        assert!(!app.world().contains_resource::<VisualTimeOverride>());
        // Any extension system would fail here: none of its required resources exist.
        app.update();
    }

    #[test]
    fn keybind_respects_existing_input_authority() {
        assert!(keybind_allowed(true, false));
        assert!(!keybind_allowed(false, false));
        assert!(!keybind_allowed(true, true));
    }

    #[test]
    fn configured_sample_drives_the_app_adapter_offline() {
        if std::env::var_os(COMPONENT_ENV).is_none() {
            eprintln!(
                "skipping configured_sample_drives_the_app_adapter_offline: fixture unavailable; requires CINNABAR_MOD_COMPONENT and installed UI carrier (make assets)"
            );
            return;
        }
        let Some(presentation) =
            crate::ui_runtime::presentation::forms::pack_harness::engine_presentation()
        else {
            return;
        };
        let mut app = App::new();
        app.insert_resource(presentation)
            .insert_resource(UiRuntime::new(1))
            .insert_resource(crate::player_runtime::PlayerRuntime::new(1))
            .insert_resource(ButtonInput::<KeyCode>::default());
        let window = app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow))
            .id();
        let vanilla = sample_frame(&mut app);
        configure_from_environment(&mut app);
        assert!(app.world().contains_resource::<ModRuntime>());
        app.update();
        let initial = sample_frame(&mut app);
        assert_ne!(vanilla, initial);

        app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(DEMO_KEY);
        app.update();
        assert_eq!(initial, sample_frame(&mut app));
        app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
        app.update();
        assert_ne!(initial, sample_frame(&mut app));
        assert!(
            app.world()
                .resource::<ButtonInput<KeyCode>>()
                .just_pressed(DEMO_KEY)
        );
    }

    /// Renders the adapter's retained state without a window or network session.
    fn sample_frame(app: &mut App) -> render_model::UiRenderInput {
        let player_runtime = app
            .world()
            .resource::<crate::player_runtime::PlayerRuntime>()
            .clone();
        app.world_mut()
            .resource_mut::<UiPresentationRuntime>()
            .build(
                &player_runtime,
                &UiRuntime::new(1),
                0,
                [1280, 720],
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap()
    }
}

#[cfg(all(test, feature = "local-mods"))]
mod time_changer_tests;

#[cfg(feature = "local-mods")]
pub(crate) mod block_highlights;
#[cfg(feature = "local-mods")]
pub(super) mod fullbright;
#[cfg(feature = "local-mods")]
mod gameplay;
#[cfg(feature = "local-mods")]
pub(crate) mod ghost;
#[cfg(feature = "local-mods")]
mod input;
#[cfg(feature = "local-mods")]
pub(crate) mod interaction;
#[cfg(feature = "local-mods")]
pub(crate) mod packet_delay;
#[cfg(feature = "local-mods")]
mod player_state;
#[cfg(feature = "local-mods")]
mod render;
