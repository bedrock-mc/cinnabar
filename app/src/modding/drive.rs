//! Applies extension observations through the shared gameplay and presentation owners.

use super::*;

/// Native presentation facts and retained observers available to one mod frame.
#[derive(SystemParam)]
pub(super) struct ModObservation<'w, 's> {
    weather: Option<ResMut<'w, crate::environment::VisualWeatherOverride>>,
    click_limiter: Local<'s, semantic_input::ClickLimiter>,
    click_debouncer: Local<'s, semantic_input::ClickDebouncer>,
    click_scope: Local<'s, Option<(u64, i32)>>,
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
pub(super) fn drive_mod(
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
        mut weather,
        mut click_limiter,
        mut click_debouncer,
        mut click_scope,
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
    if let Some(weather) = weather.as_deref_mut() {
        weather.0 = None;
    }
    let user_freelook = gameplay
        .input
        .as_ref()
        .and_then(|input| input.snapshot())
        .is_some_and(|input| input.phases[semantic_input::Action::Freelook as usize].held);
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
        click_limiter.reset();
        click_debouncer.reset();
        *click_scope = None;
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
        if let Some(camera) = camera.as_deref_mut() {
            camera.set_preserve_teleport_rotation(false);
            camera.set_hurt_camera_style(None);
            native_camera::apply(
                camera,
                gameplay.view.as_deref_mut(),
                user_freelook,
                effects.as_ref(),
            );
        }
        let delta = effects
            .as_ref()
            .and_then(|effects| effects.rotation)
            .map(|delta| mod_host::CameraDelta {
                yaw: delta.yaw,
                pitch: delta.pitch,
            });
        gameplay.apply_native_controls(effects.as_ref(), delta, &ui);
        return;
    };
    if extension.suspended {
        time_override.0 = None;
        click_limiter.reset();
        click_debouncer.reset();
        *click_scope = None;
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
            native_camera::apply(camera, gameplay.view.as_deref_mut(), user_freelook, None);
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
    let scope = world_observation
        .as_ref()
        .map(|world| (world.session, world.dimension));
    if *click_scope != scope || !captured || absorbed {
        click_limiter.reset();
        click_debouncer.reset();
    }
    *click_scope = scope;
    let physical_attack_held = gameplay
        .input
        .as_ref()
        .is_some_and(|input| input.phase(semantic_input::Action::Attack).held);
    if captured
        && !absorbed
        && scope.is_some()
        && let Some(input) = gameplay.input.as_deref_mut()
    {
        input.debounce_clicks(
            &mut click_debouncer,
            merged.click_debounce,
            time.delta_secs_f64(),
        );
    }
    interaction.attack_pulse =
        merged.attack_pulse && physical_attack_held && gameplay.pulse_attack();
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
    let delta = merged.delta.or_else(|| {
        native_effects
            .as_ref()
            .and_then(|effects| effects.rotation)
            .map(|delta| mod_host::CameraDelta {
                yaw: delta.yaw,
                pitch: delta.pitch,
            })
    });
    gameplay.apply_native_controls(
        native_effects.as_ref().filter(|_| !regular_camera),
        delta,
        &ui,
    );
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
            user_freelook,
            native_effects.as_ref().filter(|_| !regular_camera),
        );
    }
    if let Some(mut policy) = item_use {
        policy.scope = merged.item_use_delay_fix;
    }
    time_override.0 = merged.time_override;
    if let Some(weather) = weather.as_deref_mut() {
        weather.0 = merged.weather_override;
    }
    if captured
        && !absorbed
        && scope.is_some()
        && let Some(input) = gameplay.input.as_deref_mut()
    {
        input.limit_clicks(
            &mut click_limiter,
            merged.click_limits,
            time.delta_secs_f64(),
        );
    }
    if let Err(error) = presentation
        .mod_ui_mut()
        .set_mod_label(extension.hosts.merged_label())
    {
        warn!("Cinnabar extension HUD rejected: {error}");
    }
    if let Err(error) = presentation
        .mod_ui_mut()
        .set_mod_hud_sources(extension.hud_sources())
    {
        warn!("Cinnabar extension HUD sources rejected: {error}");
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
