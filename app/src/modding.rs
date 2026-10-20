//! Native local-mod composition and gameplay integration.

use client_extensions::modding::{
    font,
    grants::environment_grants,
    multi::{self, ModHosts},
};

use crate::semantic_controls::ReservedControls;
use bevy::prelude::*;
use {
    crate::{environment::VisualTimeOverride, runtime::network::resource_packs::PackApplication},
    bevy::{
        ecs::system::SystemParam,
        window::{CursorOptions, PrimaryWindow},
    },
    client_runtime::network::NetworkHandle,
    client_shell::menu::MenuRuntime,
    client_shell::ui_resources::{UiPresentationResource, UiRuntimeResource},
    client_state::ClientFrameSet,
    mod_host::{ModGrants, ModHost},
    std::{path::Path, time::Duration},
};

const COMPONENT_ENV: &str = "CINNABAR_MOD_COMPONENT";
/// A mod package directory (`mod.toml`, `mod.wasm`, templates, textures); it takes precedence
/// over a bare component.
const PACKAGE_ENV: &str = "CINNABAR_MOD_PACKAGE";
const DEMO_KEY: KeyCode = KeyCode::F8;
const RELOAD_INTERVAL: Duration = Duration::from_millis(500);

mod admission;
mod anchors;
mod drawing_policy;
mod environment_loader;
mod hud_editor;
mod library;
mod native_camera;
mod native_input;
mod packet_highlights;
mod packet_menu;
mod packet_render;
pub(crate) mod packets;
pub(crate) mod registration;
mod terrain;

/// Every loaded mod's presentation cues committed this frame, in load order.
#[derive(Resource, Default)]
pub(crate) struct ModCueFeed(pub Vec<mod_host::ModCue>);

#[derive(Resource)]
pub(crate) struct ModRuntime {
    hosts: ModHosts,
    /// One entry per host, kept in host order by the lifecycle methods.
    metadata: Vec<registration::HostMetadata>,
    /// Per-mod render generations behind the last merged scene.
    render_sources: Vec<u64>,
    render_merge: mod_render::RenderMerge,
    controls: mod_host::ControlFrame,
    suspended: bool,
    hud_editor_owner: Option<hud_editor::Owner>,
    /// The screen owner's input and layout between frames, and which mod that is.
    screens: screens::ScreenState,
}

impl ModRuntime {
    /// Starts a runtime with one host and its stable presentation metadata.
    fn new(host: ModHost, metadata: registration::HostMetadata) -> Self {
        Self {
            hosts: ModHosts::new(host, Vec::new()),
            metadata: vec![metadata],
            render_sources: Vec::new(),
            render_merge: Default::default(),
            controls: mod_host::empty_controls(),
            suspended: false,
            hud_editor_owner: None,
            screens: Default::default(),
        }
    }

    /// Appends a guest with its metadata and invalidates the merged render output.
    fn push_host(&mut self, host: ModHost, metadata: registration::HostMetadata) {
        self.hosts.push(host);
        self.metadata.push(metadata);
        self.render_sources.clear();
    }

    /// Replaces an environment guest and its presentation resources, retiring its input state.
    fn replace_host(
        &mut self,
        index: usize,
        mut prepared: admission::PreparedMod,
        presentation: &mut client_ui::ui_runtime::presentation::UiPresentationRuntime,
    ) -> ModHost {
        self.screens.replace_host(index);
        if self
            .hud_editor_owner
            .is_some_and(|owner| owner.host == index)
        {
            hud_editor::cancel(self, presentation, false);
        }
        prepared
            .host
            .activate_settings(Some(self.hosts.host_mut(index)));
        let retired = std::mem::replace(self.hosts.host_mut(index), prepared.host);
        let metadata = &mut self.metadata[index];
        metadata.font = prepared.font;
        metadata.branding = prepared.branding;
        presentation
            .mod_ui_mut()
            .set_prepared_panel_ui(metadata.owner, prepared.panel_ui);
        self.render_sources.clear();
        retired
    }

    /// The first active local mod may select ordinary input modes for this frame.
    pub(crate) fn movement_toggles(&self) -> Option<&mod_host::MovementToggles> {
        if self.suspended {
            return None;
        }
        (0..self.hosts.host_count()).find_map(|index| self.hosts.host(index).movement_toggles())
    }

    /// Moves metadata, guest and index-based input ownership together without a restart.
    fn swap_hosts(&mut self, left: usize, right: usize) {
        self.hosts.swap(left, right);
        self.metadata.swap(left, right);
        self.screens.swap_hosts(left, right);
        if let Some(owner) = self.hud_editor_owner.as_mut() {
            if owner.host == left {
                owner.host = right;
            } else if owner.host == right {
                owner.host = left;
            }
        }
        self.render_sources.clear();
    }
    /// Removes one guest and adjusts the surviving input and presentation owners together.
    fn remove_host(&mut self, index: usize) -> ModHost {
        let host = self.hosts.remove(index);
        self.metadata.remove(index);
        self.screens.remove_host(index);
        if let Some(owner) = self.hud_editor_owner.as_mut() {
            if owner.host == index {
                self.hud_editor_owner = None;
            } else if owner.host > index {
                owner.host -= 1;
            }
        }
        self.render_sources.clear();
        self.controls = mod_host::empty_controls();
        host
    }
    /// Maps a publishing host's runtime index to its stable panel catalog owner.
    fn panel_ui_owner(&self, host: usize) -> mod_ui::PanelUiOwner {
        self.metadata[host].owner
    }

    /// Existing mod order chooses one active, explicitly authorized local menu brand.
    fn menu_branding(
        &self,
    ) -> Option<
        std::sync::Arc<client_ui::ui_runtime::presentation::menu_branding::PreparedMenuBranding>,
    > {
        self.metadata
            .iter()
            .enumerate()
            .find_map(|(index, metadata)| {
                let host = self.hosts.host(index);
                (host.is_active() && host.grants().environment)
                    .then(|| metadata.branding.clone())
                    .flatten()
            })
    }
}

/// Bounded personal-extension diagnostics, only through the developer control endpoint.
#[cfg(feature = "developer-control")]
pub(crate) fn developer_state(world: &World) -> Option<serde_json::Value> {
    let runtime = world.get_resource::<ModRuntime>()?;
    let hosts: Vec<_> = (0..runtime.hosts.host_count())
        .map(|index| {
            let host = runtime.hosts.host(index);
            serde_json::json!({
                "index": index,
                "active": host.is_active(),
                "label": host.label(),
                "panel_open": host.panel_open(),
                "reserved_keys": host.reserved_keys(),
                "movement_granted": host.grants().movement,
                "refused_outputs": host.refused_outputs(),
                "last_fuel_used": host.last_fuel_used(),
                "hud_cards": host.hud().map_or(0, |hud| hud.cards.len()),
                "hud": host.hud(),
                "panel": host.panel(),
            })
        })
        .collect();
    Some(serde_json::json!({ "suspended": runtime.suspended, "hosts": hosts }))
}

/// Installs the developer extension only when its component path is explicit.
pub(crate) fn configure_from_environment(app: &mut App) {
    let package = std::env::var_os(PACKAGE_ENV);
    let path = std::env::var_os(COMPONENT_ENV);
    if let Some(set) = std::env::var_os(multi::SET_ENV) {
        match multi::read_set(Path::new(&set)) {
            Ok(mods) => configure_entries(app, mods),
            Err(error) => warn!("Local mod set disabled: {error}"),
        }
    } else if let Some(package) = package.as_deref() {
        configure_package(app, Path::new(package));
    } else if let Some(path) = path.as_deref() {
        configure(app, Some(Path::new(path)));
    }
    {
        let configured = app.world().contains_resource::<ModRuntime>();
        let layout =
            launcher::install_layout::InstallLayout::discover().map_err(|error| error.to_string());
        match layout.and_then(|layout| {
            registration::Watcher::start(layout.user_config_root.clone())
                .map(|watcher| (layout, watcher))
        }) {
            Ok((layout, watcher)) => {
                library::configure(
                    app,
                    layout.mods_dir(),
                    layout.user_config_root,
                    watcher.removal_client(),
                );
                app.insert_resource(watcher);
                if configured {
                    configure_registration_systems(app);
                } else {
                    configure_systems(app, true);
                }
            }
            Err(error) => {
                warn!("Local extension watcher unavailable: {error}");
                if !configured && app.world().contains_resource::<packets::PacketMods>() {
                    configure_systems(app, false);
                }
            }
        }
    }
}

/// Loads one optional component without changing the vanilla schedule on absence.
fn configure(app: &mut App, path: Option<&Path>) {
    configure_with_grants(app, path, environment_grants());
}

/// Loads one package with the developer profile: what its manifest asks for, plus the explicit
/// opt-ins a bare component gets.
fn configure_package(app: &mut App, dir: &Path) {
    match ModHost::load_package(dir, environment_grants()) {
        Ok(host) => install(app, vec![host]),
        Err(error) => warn!("Cinnabar mod package {} disabled: {error:#}", dir.display()),
    }
}

/// Grants are explicit and apply only to the selected personal component.
fn configure_with_grants(app: &mut App, path: Option<&Path>, grants: ModGrants) {
    if let Some(path) = path {
        configure_set(app, vec![(path.to_owned(), grants)]);
    }
}

/// Loads components in order, each with its own grants; one failing to load leaves the rest.
fn configure_set(app: &mut App, mods: Vec<(std::path::PathBuf, ModGrants)>) {
    let entries = mods
        .into_iter()
        .map(|(path, grants)| (multi::ModSource::Component(path), grants));
    configure_entries(app, entries.collect());
}

/// Loads components and packages in order, each with its own grants; one failing to load
/// leaves the rest.
fn configure_entries(app: &mut App, mods: Vec<(multi::ModSource, ModGrants)>) {
    if let Some(selection) = multi::packet_mod_entry(&mods) {
        app.world_mut().init_resource::<packets::PacketMods>();
        app.world_mut()
            .resource_mut::<packets::PacketMods>()
            .select_environment(selection);
    }
    install(app, multi::load_entries(mods));
}

/// Installs loaded mods in load order; none installs nothing.
fn install(app: &mut App, mut hosts: Vec<ModHost>) {
    if let Some(path) = std::env::var_os(client_extensions::modding::panel_ui::UI_ENV)
        && let Some(host) = hosts.iter_mut().find(|host| host.grants().controls)
    {
        let result =
            client_extensions::modding::panel_ui::load(Path::new(&path)).and_then(|json| {
                host.set_panel_bundle(&json)
                    .map_err(|error| format!("{error:#}"))
            });
        if let Err(error) = result {
            warn!("Optional personal-panel catalog unavailable: {error}");
        }
    }
    let override_font =
        std::env::var_os(font::FONT_ENV).and_then(|path| match font::load(Path::new(&path)) {
            Ok(font) => Some(std::sync::Arc::new(font)),
            Err(error) => {
                warn!("Optional personal-panel font unavailable: {error}");
                None
            }
        });
    let hosts: Vec<_> = hosts
        .into_iter()
        .filter_map(|host| {
            let result = if host.grants().controls {
                if let Some(font) = &override_font {
                    Ok(Some(font.clone()))
                } else {
                    host.panel_font_source()
                        .map(font::rasterize)
                        .transpose()
                        .map(|font| font.map(std::sync::Arc::new))
                }
            } else {
                Ok(None)
            };
            match result {
                Ok(font) => match admission::prepare_branding(&host) {
                    Ok(branding) => Some((host, font, branding)),
                    Err(error) => {
                        warn!("Embedded menu branding rejected; component disabled: {error}");
                        None
                    }
                },
                Err(error) => {
                    warn!("Embedded personal-panel font rejected; component disabled: {error}");
                    None
                }
            }
        })
        .enumerate()
        .map(|(index, (host, font, branding))| {
            let mut metadata = registration::HostMetadata::environment(index);
            metadata.font = font;
            metadata.branding = branding;
            (host, metadata)
        })
        .collect();
    for (host, metadata) in &hosts {
        if let Some(bundle) = host.panel_bundle() {
            match mod_ui::PreparedPanelUi::prepare(bundle) {
                Ok(prepared) => {
                    if let Some(mut presentation) =
                        app.world_mut().get_resource_mut::<UiPresentationResource>()
                    {
                        presentation
                            .mod_ui_mut()
                            .set_prepared_panel_ui(metadata.owner, Some(prepared));
                    }
                }
                Err(error) => warn!("Optional personal-panel preparation unavailable: {error}"),
            }
        }
    }
    let first_controls = hosts.iter().position(|(host, _)| host.grants().controls);
    let sources = hosts
        .iter()
        .enumerate()
        .map(|(index, (host, metadata))| {
            let mut source = environment_loader::Source::from_host(metadata.owner, host);
            source.font = std::env::var_os(font::FONT_ENV).map(std::path::PathBuf::from);
            source.ui = (Some(index) == first_controls)
                .then(|| std::env::var_os(client_extensions::modding::panel_ui::UI_ENV))
                .flatten()
                .map(std::path::PathBuf::from);
            source
        })
        .collect::<Vec<_>>();
    if !sources.is_empty() {
        match environment_loader::Loader::start(sources) {
            Ok(loader) => {
                app.insert_resource(loader);
            }
            Err(error) => warn!("Environment component worker unavailable: {error}"),
        }
    }
    let mut hosts = hosts.into_iter();
    let Some((host, metadata)) = hosts.next() else {
        return;
    };
    app.init_resource::<client_presentation::actor_publication::ActorPresentationStyle>();
    app.insert_resource(VisualTimeOverride(host.time_override()));
    let mut runtime = ModRuntime::new(host, metadata);
    for (host, metadata) in hosts {
        runtime.push_host(host, metadata);
    }
    app.insert_resource(runtime)
        .init_resource::<interaction::ModInteraction>();
    registration::presentation::refresh_presentation(app.world_mut());
    configure_systems(app, false);
}

fn configure_systems(app: &mut App, watching: bool) {
    app.init_resource::<native_input::NativeInput>();
    app.init_resource::<packet_render::Drawing>();
    app.init_resource::<ReservedControls>();
    app.init_resource::<client_presentation::actor_publication::ActorPresentationStyle>();
    app.add_systems(Update, environment_loader::poll.before(begin_mod_frame));
    ghost::configure(app);
    app.add_systems(
        Update,
        anchors::publish
            .after(ClientFrameSet::ActorPublication)
            .after(drive_mod)
            .before(ClientFrameSet::UiPublication),
    );
    block_highlights::configure(app);
    fullbright::configure(app);
    app.init_resource::<crate::item_use::ModItemUsePolicy>()
        .init_resource::<ModCueFeed>()
        .add_plugins(::render::ModRenderPlugin)
        .add_systems(Update, render::grant_depth_sampling);
    if watching {
        configure_registration_systems(app);
    }
    app.add_systems(
        Update,
        (begin_mod_frame, input::prepare_mod_input)
            .chain()
            .before(ClientFrameSet::RawInput),
    );
    app.add_systems(
        Update,
        drive_mod
            .in_set(client_state::ModCameraInputSet)
            .after(ClientFrameSet::SemanticFinalize)
            .before(ClientFrameSet::UiPublication)
            .before(crate::environment::update_atmosphere_frame),
    );
    app.init_resource::<packet_delay::RealPositionSnapshot>()
        .add_systems(Update, packet_delay::publish_packet_delay.after(drive_mod))
        .add_systems(Update, inventory::apply_inventory_requests.after(drive_mod));
}

/// Starts the shared callback allowance before any input or event reaches a guest.
fn begin_mod_frame(
    extension: Option<ResMut<ModRuntime>>,
    gameplay: gameplay::GameplayContext,
    actors: Option<Res<render_scene::ActorRenderFrame>>,
    mut drawing: ResMut<packet_render::Drawing>,
) {
    if drawing.needs_targets() {
        drawing.targets(
            actors
                .as_deref()
                .map(render_scene::mod_scene::mask_targets)
                .unwrap_or_default(),
        );
    }
    if let Some(mut extension) = extension {
        drawing_policy::bind(&mut extension.hosts, &gameplay);
        anchors::reconcile(&mut extension.hosts, &gameplay);
        let needs_targets = (0..extension.hosts.host_count())
            .any(|index| extension.hosts.host(index).grants().entity_highlight);
        let targets = actors
            .as_deref()
            .filter(|_| needs_targets)
            .map(render_scene::mod_scene::mask_targets)
            .unwrap_or_default();
        for index in 0..extension.hosts.host_count() {
            extension
                .hosts
                .host_mut(index)
                .publish_mask_targets(&targets);
        }
        extension.hosts.begin_frame();
    }
}

/// Flushes registration transitions before the shared physical-input router.
fn configure_registration_systems(app: &mut App) {
    app.add_systems(
        Update,
        (registration::install_pending, ApplyDeferred)
            .chain()
            .before(begin_mod_frame),
    );
}

/// Runs the bounded guest and publishes only its validated presentation output.
/// Where a mod's network, camera and cue output lands.
type ModOutputs<'w> = (
    Option<Res<'w, NetworkHandle<PackApplication>>>,
    Option<ResMut<'w, client_presentation::camera::CameraSettingsAuthority>>,
    Option<ResMut<'w, ModCueFeed>>,
    Option<ResMut<'w, crate::item_use::ModItemUsePolicy>>,
);

/// Native presentation facts and retained observers available to one mod frame.
#[derive(SystemParam)]
struct ModObservation<'w, 's> {
    packets: Option<Res<'w, packets::PacketMods>>,
    watcher: Option<Res<'w, registration::Watcher>>,
    render_scene: Option<ResMut<'w, render_scene::ModRenderScene>>,
    drawing: Option<ResMut<'w, packet_render::Drawing>>,
    cameras: Query<
        'w,
        's,
        (&'static GlobalTransform, &'static Projection),
        With<client_presentation::camera::FlyCamera>,
    >,
    frame_rate: Local<'s, presentation_facts::FrameRate>,
    module_observer: Local<'s, module_observations::ModuleObserver>,
    terrain: Local<'s, terrain::TerrainSampling>,
    actor_style: Option<ResMut<'w, client_presentation::actor_publication::ActorPresentationStyle>>,
}

#[allow(
    clippy::too_many_arguments,
    reason = "Player authority is borrowed separately from UI state."
)]
fn drive_mod(
    player_runtime: bevy::prelude::Res<client_state::PlayerRuntime>,
    extension: Option<ResMut<ModRuntime>>,
    keys: Res<ButtonInput<KeyCode>>,
    focus: Option<Res<client_state::CursorFocus>>,
    driven: Option<Res<client_state::DrivenInput>>,
    windows: Query<(&Window, Option<&CursorOptions>), With<PrimaryWindow>>,
    mut ui: ResMut<UiRuntimeResource>,
    menu: Option<Res<MenuRuntime>>,
    mut presentation: ResMut<UiPresentationResource>,
    time_override: Option<ResMut<VisualTimeOverride>>,
    mut gameplay: gameplay::GameplayContext,
    interaction: Option<ResMut<interaction::ModInteraction>>,
    outputs: ModOutputs,
    mut screen_input: screens::ScreenInput,
    time: Res<Time<Real>>,
    observation: ModObservation,
) {
    let ModObservation {
        packets,
        watcher,
        mut render_scene,
        mut drawing,
        cameras,
        mut frame_rate,
        mut module_observer,
        mut terrain,
        mut actor_style,
    } = observation;
    let (network, mut camera, cues, item_use) = outputs;
    let focused = windows.single().is_ok_and(|(window, _)| window.focused);
    let absorbed = client_shell::screen_policy::absorbs_input(
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
            && (0..runtime.hosts.host_count()).any(|index| {
                runtime.hosts.host(index).is_active() && runtime.hosts.host(index).grants().movement
            })
    });
    gameplay.synchronize_jump_scope(movement_allowed && captured && !absorbed, &ui);
    let menu_open = menu.as_deref().is_some_and(MenuRuntime::is_visible);
    screen_input.modifiers(focused && !menu_open);
    let (Some(mut extension), Some(mut time_override), Some(mut interaction)) =
        (extension, time_override, interaction)
    else {
        let effects = packets
            .as_ref()
            .filter(|packets| packets.native_frame_available())
            .and_then(|packets| {
                packets.native_frame(gameplay.native_frame(captured && !absorbed, &ui))
            });
        if let Some(drawing) = drawing.as_deref_mut() {
            drawing.update(
                effects.as_ref().and_then(|effects| effects.drawing.clone()),
                packet_render::session(&gameplay),
            );
            if let Some(scene) = render_scene.as_deref_mut() {
                packet_render::publish_alone(scene, drawing);
            }
        }
        let effects = effects.and_then(|effects| effects.camera);
        gameplay.apply_native_look(effects.as_ref());
        gameplay.apply_native_input(effects.as_ref(), &ui);
        if let Some(camera) = camera.as_deref_mut() {
            camera.set_preserve_teleport_rotation(false);
            camera.set_hurt_camera_style(None);
            native_camera::apply(camera, gameplay.view.as_deref_mut(), effects.as_ref());
        }
        if let Some(delta) = effects.and_then(|effects| effects.rotation) {
            gameplay.apply(mod_host::CameraDelta {
                yaw: delta.yaw,
                pitch: delta.pitch,
            });
        }
        return;
    };
    if extension.suspended {
        gameplay.apply_native_input(None, &ui);
        if let Some(drawing) = drawing.as_deref_mut() {
            drawing.update(None, packet_render::session(&gameplay));
            if let Some(scene) = render_scene.as_deref_mut() {
                packet_render::publish_alone(scene, drawing);
            }
        }
        if let Some(packets) = packets.as_ref() {
            packets.native_frame(None);
        }
        if let Some(camera) = camera.as_deref_mut() {
            camera.set_preserve_teleport_rotation(false);
            camera.set_hurt_camera_style(None);
            native_camera::apply(camera, gameplay.view.as_deref_mut(), None);
        }
        return;
    }
    drawing_policy::bind(&mut extension.hosts, &gameplay);
    anchors::reconcile(&mut extension.hosts, &gameplay);
    // Reconcile screen departures before frame callbacks retry deferred edits.
    let cursor = windows
        .single()
        .ok()
        .and_then(|(window, _)| window.cursor_position())
        .map(|point| point.to_array());
    screens::drive(
        &mut extension,
        &player_runtime,
        &ui,
        &mut presentation,
        menu.as_deref(),
        cursor,
        focused && !menu_open,
        &mut screen_input,
    );
    let pressed = keybind_allowed(focused, absorbed) && keys.just_pressed(DEMO_KEY);
    let controls = std::mem::replace(&mut extension.controls, mod_host::empty_controls());
    let previous_cues = cues.as_ref().map_or_else(Vec::new, |feed| feed.0.clone());
    let registrations: Vec<_> = extension
        .metadata
        .iter()
        .map(|metadata| metadata.registration.clone())
        .collect();
    let player_state = gameplay.player_state(
        (0..extension.hosts.host_count()).any(|index| {
            let host = extension.hosts.host(index);
            host.is_active() && host.grants().player_state
        }),
        &player_runtime,
        &ui,
    );
    let world_observation = gameplay.world_observation(&ui);
    let modules = module_observer.capture(
        &gameplay,
        &ui,
        world_observation.as_ref(),
        &player_runtime,
        Some(&interaction),
    );
    let chat = if world_observation.is_some() {
        presentation_facts::chat_snapshot(&ui)
    } else {
        Vec::new()
    };
    let fps = frame_rate.sample(std::time::Instant::now());
    let sidebar = world_observation
        .as_ref()
        .and_then(|_| presentation.sidebar_snapshot(&ui))
        .map(|(title, rows)| mod_host::SidebarSnapshot {
            title,
            rows: rows
                .into_iter()
                .map(|(label, score)| mod_host::SidebarRow { label, score })
                .collect(),
        });
    terrain.refresh(&mut extension.hosts, &gameplay, world_observation.as_ref());
    let mut callback_failed = false;
    let merged = multi::run_frame(
        &mut extension.hosts,
        multi::FrameInput {
            pressed,
            controls: &controls,
            previous_cues: &previous_cues,
            player_state: player_state.as_ref(),
            world_observation: world_observation.as_ref(),
            fps,
            modules: Some(&modules),
            sidebar: sidebar.as_ref(),
            chat: &chat,
        },
        |grants| {
            let snapshot = gameplay.snapshot(captured && !absorbed, grants);
            let mobs = gameplay.mobs(snapshot.as_ref(), grants);
            let movement = gameplay.movement_snapshot(captured && !absorbed, grants, &ui);
            (snapshot, mobs, movement)
        },
        |index, error| {
            callback_failed = true;
            if let Some(registered) = &registrations[index]
                && let Some(watcher) = watcher.as_ref()
            {
                watcher.quarantine(&registered.entry, error.clone());
            }
            warn!("Cinnabar extension callback failed: {error}");
        },
    );
    for index in 0..extension.hosts.host_count() {
        for message in extension.hosts.host_mut(index).take_local_messages() {
            ui.push_local_chat_line(
                message.into(),
                u64::try_from(time.elapsed().as_millis()).unwrap_or(u64::MAX),
            );
        }
    }

    if let Some(watcher) = watcher.as_ref() {
        watcher.remember_settings(Some(&extension));
    }
    if callback_failed || merged.jump_cancel {
        gameplay.synchronize_jump_scope(false, &ui);
    }
    let movement_allowed = (0..extension.hosts.host_count()).any(|index| {
        extension.hosts.host(index).is_active() && extension.hosts.host(index).grants().movement
    });
    gameplay.synchronize_jump_scope(movement_allowed && captured && !absorbed, &ui);
    gameplay.pulse_jump(merged.jump_pulse && !merged.jump_cancel, &ui);
    interaction.attack_reach = merged.attack_reach;
    interaction.attack_pulse = merged.attack_pulse && gameplay.pulse_attack();
    let regular_camera = (0..extension.hosts.host_count()).any(|index| {
        let host = extension.hosts.host(index);
        host.is_active() && host.has_camera_output()
    });
    let native_effects = packets
        .as_ref()
        .filter(|packets| packets.native_frame_available())
        .and_then(|packets| {
            packets
                .native_frame(gameplay.native_frame(captured && !absorbed && !regular_camera, &ui))
        });
    if let Some(drawing) = drawing.as_deref_mut() {
        drawing.update(
            native_effects
                .as_ref()
                .and_then(|effects| effects.drawing.clone()),
            packet_render::session(&gameplay),
        );
    }
    render::publish(render_scene, &mut extension, drawing.as_deref());
    let native_effects = native_effects.and_then(|effects| effects.camera);
    gameplay.apply_native_look(native_effects.as_ref().filter(|_| !regular_camera));
    gameplay.apply_native_input(native_effects.as_ref().filter(|_| !regular_camera), &ui);
    if let Some(delta) = merged.delta.or_else(|| {
        native_effects
            .as_ref()
            .and_then(|effects| effects.rotation)
            .map(|delta| mod_host::CameraDelta {
                yaw: delta.yaw,
                pitch: delta.pitch,
            })
    }) {
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
        camera.set_hurt_camera_style(merged.hurt_style);
        native_camera::apply(
            &mut camera,
            gameplay.view.as_deref_mut(),
            native_effects.as_ref().filter(|_| !regular_camera),
        );
    }
    if let Some(mut policy) = item_use {
        policy.scope = merged.item_use_delay_fix;
    }
    time_override.0 = merged.time_override;
    if let Err(error) = presentation
        .mod_ui_mut()
        .set_mod_label(extension.hosts.merged_label())
    {
        warn!("Cinnabar extension HUD rejected: {error}");
    }
    let combined_hud = extension.hosts.merged_hud();
    let hud = combined_hud.as_deref();
    if let Some(style) = actor_style.as_mut() {
        style.attackable_color = hud
            .and_then(|hud| hud.actor_style.as_ref())
            .and_then(|style| style.attackable_color);
        style.attackable_actor = if style.attackable_color.is_some() {
            module_observations::current_target(&gameplay, &ui, Some(&interaction)).flatten()
        } else {
            None
        };
        style.hurt_armor_color = hud
            .and_then(|hud| hud.actor_style.as_ref())
            .and_then(|style| style.hurt_armor_color);
        style.hurt_color = hud
            .and_then(|hud| hud.actor_style.as_ref())
            .and_then(|style| style.hurt_color);
    }
    if let Err(error) = presentation.set_shared_mod_hud(combined_hud) {
        warn!("Cinnabar extension HUD cards rejected: {error}");
    }
    presentation
        .mod_ui_mut()
        .set_mod_world_projection(cameras.single().ok().map(|(transform, projection)| {
            (projection.get_clip_from_view() * transform.to_matrix().inverse()).to_cols_array()
        }));
    let crosshair_owner = extension.hosts.crosshair_owner();
    let crosshair = crosshair_owner.and_then(|owner| extension.hosts.host(owner).crosshair());
    if let Err(error) = presentation.mod_ui_mut().set_mod_crosshair(crosshair) {
        warn!("Cinnabar extension crosshair rejected: {error}");
    }
    if !packets
        .as_ref()
        .is_some_and(|packets| packets.menu_owns_panel())
    {
        let owner = extension.hosts.panel_owner();
        presentation.set_mod_menu_branding(extension.menu_branding());
        if let Err(error) = presentation.set_mod_panel_font(extension.metadata[owner].font.clone())
        {
            warn!("Cinnabar extension panel font rejected: {error}");
        }
        if let Err(error) = presentation.mod_ui_mut().set_mod_panel(
            extension.panel_ui_owner(owner),
            extension.hosts.host(owner).panel(),
        ) {
            warn!("Cinnabar extension panel rejected: {error}");
            extension.hosts.host_mut(owner).set_panel_open(false);
        }
        presentation
            .mod_ui_mut()
            .set_mod_panel_open(extension.hosts.host(owner).panel_open());
    }
    let editor_session = gameplay.hud_editor_session(&ui);
    hud_editor::publish(
        &mut extension,
        &mut presentation,
        editor_session,
        focused || driven.is_some(),
    );
    if let Some(owner) = extension.hosts.screen_owner() {
        screens::publish(extension.hosts.host(owner), &mut presentation);
    } else {
        presentation.set_mod_screens(None);
    }
}

/// Granted commands travel the session-fenced UI packet lane as vanilla command requests.
fn send_commands(
    network: Option<&NetworkHandle<PackApplication>>,
    session: u64,
    commands: Vec<String>,
) {
    let Some(network) = network else { return };
    for command in commands {
        if network
            .send_form_packet(session, protocol::command_request_packet(&command))
            .is_err()
        {
            warn!("Cinnabar extension command dropped: the session is not accepting packets");
            return;
        }
    }
}

fn camera_rig(rig: mod_host::GameplayCameraRig) -> client_presentation::camera::CameraRig {
    client_presentation::camera::CameraRig {
        offset: Vec3::new(rig.offset.x, rig.offset.y, rig.offset.z),
        roll_radians: rig.roll,
        fov_delta_degrees: rig.fov_delta,
    }
}

/// A mod keybind is unavailable while another UI or an unfocused window owns input.
fn keybind_allowed(window_focused: bool, input_absorbed: bool) -> bool {
    window_focused && !input_absorbed
}

#[cfg(test)]
mod tests {
    use super::*;
    use client_ui::ui_runtime::UiRuntime;

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
        let Some(presentation) = client_ui::test_support::pack_harness::engine_presentation()
        else {
            return;
        };
        let mut app = App::new();
        app.insert_resource(UiPresentationResource(presentation))
            .insert_resource(UiRuntimeResource(UiRuntime::new(1)))
            .insert_resource(client_state::PlayerRuntime(
                ::player_state::PlayerState::new(1),
            ))
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
            .resource::<client_state::PlayerRuntime>()
            .clone();
        app.world_mut()
            .resource_mut::<UiPresentationResource>()
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

#[cfg(test)]
mod time_changer_tests;

pub(crate) mod block_highlights;
pub(super) mod fullbright;
mod gameplay;
pub(crate) mod ghost;
mod input;
use crate::interaction_override as interaction;
mod inventory;
mod module_observations;
pub(crate) mod packet_delay;
mod render;
mod screens;

mod presentation_facts;
