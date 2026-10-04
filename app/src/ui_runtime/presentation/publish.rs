//! Per-frame HUD observation and publication.
use super::*;
use bevy::prelude::Transform;

mod commit;
use client_ui::ui_runtime::presentation::{
    ItemIconFrames, PendingUiPublication, PreparedUiPublication, PreviewCapture, capture_hud_frame,
    nametags, player_preview, startup::StartupReadinessInput,
};
pub(crate) use commit::publish_ui_runtime;

pub(crate) fn observe_mount_jump_input(
    input: Res<crate::semantic_controls::SemanticInputSnapshot>,
    mut runtime: ResMut<UiRuntime>,
    time: Res<Time<Real>>,
) {
    let now_millis = u64::try_from(time.elapsed().as_millis()).unwrap_or(u64::MAX);
    runtime.set_mount_jump_held(input.phase(semantic_input::Action::Jump).held, now_millis);
}

pub(crate) fn platform_safe_area_insets() -> SafeArea {
    SafeArea::ZERO
}
/// Resources beyond Bevy's sixteen-parameter limit.
type PublishExtras<'w> = (
    Res<'w, WorldStreamFramePoll>,
    Res<'w, crate::menu::MenuRuntime>,
    Res<'w, render::HandRigScene>,
    Option<Res<'w, crate::movement::PhysicsCollisionRegistries>>,
    Option<Res<'w, render::RuntimeStageProfiler>>,
    (
        Res<'w, crate::runtime::network::ActorFramePartialTick>,
        Res<'w, crate::local_player::LocalPlayerFrameCarrier>,
        Res<'w, crate::environment::WorldClock>,
        Res<'w, crate::environment::WeatherState>,
        Res<'w, crate::runtime::network::NetworkHandle>,
        Option<ResMut<'w, render::UiGlintSettings>>,
        Res<'w, crate::item_use::ItemUseRuntime>,
        Res<'w, crate::movement::MovementTicker>,
        Res<'w, crate::movement::LocalPhysicsController>,
    ),
);

/// Observes UI authority and captures inventory before outbound actions mutate it.
#[allow(clippy::too_many_arguments)]
pub(crate) fn prepare_ui_runtime(
    player_runtime: bevy::prelude::Res<crate::player_runtime::PlayerRuntime>,
    mut runtime: ResMut<UiRuntime>,
    mut presentation: ResMut<UiPresentationRuntime>,
    mut prepared: ResMut<PreparedUiPublication>,
    visibility: Res<CaveVisibilityCache>,
    mut diagnostics_input: ResMut<VisibilityDiagnosticsInput>,
    visibility_diagnostics: Res<VisibilityDiagnostics>,
    render_queue: Res<ChunkRenderQueue>,
    upload_acknowledgements: Res<ChunkUploadAcknowledgements>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut client_world: ResMut<ClientWorld>,
    camera_settings: Res<CameraSettingsAuthority>,
    // The camera's Transform is this frame's; its GlobalTransform is propagated after Update.
    cameras: Query<(&Camera, &Transform), With<Camera3d>>,
    time: Res<Time<Real>>,
    (
        frame_poll,
        menu_runtime,
        hand_rig,
        collisions,
        profiler,
        (
            actor_partial,
            local_frame,
            clock,
            weather,
            network,
            glint_settings,
            item_use,
            movement,
            physics,
        ),
    ): PublishExtras,
    mut hand: crate::presentation::viewmodel::ViewmodelPublish,
) {
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(render::RuntimeStage::UiPreparation));
    prepared.0 = None;
    runtime.set_toast_lifetime(menu_runtime.settings_snapshot().0.toast_lifetime_millis());
    if let Some(mut glint_settings) = glint_settings {
        *glint_settings = menu_runtime.ui_glint_settings();
    }
    let Ok(window) = windows.single() else {
        hand.clear();
        return;
    };
    let physical_size = [window.physical_width(), window.physical_height()];
    if physical_size.contains(&0) {
        hand.clear();
        return;
    }
    let logical_width = physical_size[0] as f32 / window.scale_factor();
    let logical_height = physical_size[1] as f32 / window.scale_factor();
    let Ok(dpi_scale) = DpiScale::new(window.scale_factor()) else {
        hand.clear();
        record_fatal_error(
            &mut client_world.fatal_error,
            "primary window reported an unsupported UI DPI scale".to_owned(),
        );
        return;
    };
    let now_millis = u64::try_from(time.elapsed().as_millis()).unwrap_or(u64::MAX);
    runtime.expire_hud(now_millis);
    if menu_runtime.is_visible() {
        presentation.set_loading_stage(None);
        diagnostics_input.set_startup_probe_enabled(false);
    } else {
        let (connected, stream_work_drained) =
            client_world
                .stream
                .as_ref()
                .map_or((false, false), |stream| {
                    let stats = stream.stats();
                    let drained = stats.queued_decode_jobs == 0
                        && stats.in_flight_decode_jobs == 0
                        && stats.pending_light_jobs == 0
                        && stats.in_flight_light_jobs == 0
                        && stats.pending_mesh_jobs == 0
                        && stats.in_flight_mesh_jobs == 0
                        && stats.pending_retry_requests == 0
                        && stats.awaiting_sub_chunk_responses == 0
                        && stats.admitted_world_events == 0
                        && stats.admitted_heavy_events == 0
                        && stream.pending_request_work_count() == 0
                        && stream.outstanding_sub_chunk_count() == 0
                        && stream.pending_mesh_change_count() == 0
                        && stream.unacknowledged_mesh_count() == 0;
                    (true, drained)
                });
        let render_work_drained =
            render_queue.retained_len() == 0 && upload_acknowledgements.is_empty();
        let loading = presentation.startup_mut().probe_enabled(connected);
        let (startup_released, loading_milestone) =
            presentation.startup_mut().observe_with_milestone(
                StartupReadinessInput {
                    session_generation: runtime.session_id(),
                    connected,
                    diagnostics_frame_generation: diagnostics_input.frame_generation(),
                    snapshot: visibility_diagnostics.snapshot(),
                    visible_rendered: visibility.visible_rendered,
                    local_terrain_ready: loading
                        && client_world
                            .stream
                            .as_ref()
                            .is_some_and(chunk_pipeline::WorldStream::local_terrain_ready),
                    cohort_target_complete: frame_poll.cohort.map_or_else(
                        // Outside acceptance runs the cohort is only scanned while loading, so a
                        // sparse view (a Flat world) can still release the loading screen. A
                        // server that sent no terrain before spawn (Dragonfly) sends none until
                        // initialized, so its empty startup view releases once work drains.
                        || {
                            loading
                                && client_world
                                    .stream
                                    .as_ref()
                                    .is_some_and(|stream| stream.startup_view_complete())
                        },
                        |status| status.target_is_complete(),
                    ),
                    stream_work_drained,
                    render_work_drained,
                    world_entry_held: runtime.experiences.holds_world_entry(),
                },
                now_millis,
            );
        if let Some(milestone) = loading_milestone {
            eprintln!("{milestone}");
        }
        diagnostics_input
            .set_startup_probe_enabled(presentation.startup_mut().probe_enabled(connected));
        presentation.set_loading_stage(if !connected {
            Some(LoadingStage::Connecting)
        } else if startup_released {
            None
        } else {
            Some(LoadingStage::BuildingTerrain)
        });
        if startup_released && !presentation.startup_mut().completion_queued {
            presentation.startup_mut().completion_queued = network.finish_loading();
        }
    }
    runtime.expire_gameplay_effects(now_millis);
    let stream = client_world.stream.as_ref();
    let menu_skin = menu_runtime.player_skin();
    let skin = player_preview::local_preview_skin(
        stream,
        &render::ActorSkinPixels {
            width: menu_skin.width,
            height: menu_skin.height,
            rgba8: menu_skin.rgba8.clone(),
        },
    );
    let pose = player_preview::PlayerPreviewPose::of_local_player(stream);
    // The model wears the local player's armor and held item.
    presentation.dress_player_preview(&player_runtime, &runtime, |stack| {
        client_world
            .stream
            .as_ref()?
            .authority()
            .canonical_item_stack(stack)?
            .identifier
    });
    let doll_state = client_world
        .stream
        .as_ref()
        .and_then(|stream| observe_paper_doll(stream, &runtime, &player_runtime, &physics));
    presentation.observe_paper_doll(now_millis, doll_state);
    let settings = menu_runtime.settings_snapshot().0;
    let hud_doll = presentation.hud_frame_mut().paper_doll_visible
        && settings.value("hide_hud") == 0
        && settings.value("hide_paperdoll") == 0
        && !menu_runtime.is_visible()
        && !runtime.inventory_open();
    if hud_doll {
        presentation.capture_hud_player(
            client_world.stream.as_ref(),
            doll_state.is_some_and(|state| state.swimming),
        );
    }
    let hide_hand = settings.value("hide_hand") != 0;
    // The paper doll shows in the inventory and menus; the CPU hands only while no GPU hand rig.
    let first_person =
        camera_settings.perspective() == semantic_input::PerspectiveMode::FirstPerson;
    let preview = PreviewCapture {
        skin,
        pose,
        shown: runtime.inventory_open() || menu_runtime.is_visible() || hud_doll,
        hands: first_person && !hide_hand && !hand_rig.is_active(),
    };
    client_ui::ui_runtime::presentation::forms::observe_station_block(
        &player_runtime,
        &mut runtime,
        client_world.stream.as_ref(),
        |position| {
            station_triggered(
                client_world.stream.as_ref()?,
                collisions.as_deref()?,
                position,
            )
        },
        now_millis,
    );
    let icon_frames = ItemIconFrames(std::array::from_fn(|slot| {
        client_world.stream.as_ref().and_then(|stream| {
            item_use.inventory_animation_frame(
                &player_runtime,
                stream,
                &runtime,
                slot as u8,
                movement.completed_tick(),
            )
        })
    }));
    let item_icons = capture_hud_frame(
        &player_runtime,
        &mut runtime,
        &mut presentation,
        client_world.stream.as_ref(),
        camera_settings.perspective(),
        now_millis,
        icon_frames,
    );
    // Floored feet position and absolute world tick for the HUD's position and days-played text.
    presentation.hud_frame_mut().player_block = local_frame.snapshot().map(|frame| {
        let feet = frame.pose().translation;
        [feet.x, feet.y, feet.z].map(|axis| axis.floor() as i32)
    });
    presentation.hud_frame_mut().thunderstorm = weather.lightning_level() > 0.0;
    presentation.hud_frame_mut().dimension = client_world
        .stream
        .as_ref()
        .map_or(0, |stream| stream.current_dimension());
    presentation.hud_frame_mut().world_time = Some(crate::environment::visual_world_time(
        *clock,
        time.elapsed_secs_f64(),
    ));
    // When the local player's first-person rig is drawing near-camera, it owns the hand; the
    // static empty-hand scene and the HUD's CPU hand/item carriers are retired so nothing
    // double-draws.
    presentation.hud_frame_mut().first_person &= !hide_hand;
    presentation.hud_frame_mut().hand_rig_active = hand_rig.is_active();
    if hand_rig.is_active() {
        hand.use_animated_rig();
    } else {
        hand.observe(
            &player_runtime,
            &runtime,
            &client_world,
            presentation.hud_frame_mut().first_person,
            hide_hand
                || !presentation.renders_game_behind(&player_runtime, &runtime, &*menu_runtime)
                || presentation.loading_stage().is_some(),
            physical_size,
        );
    }
    let show_names = menu_runtime
        .settings_snapshot()
        .0
        .value("ingame_player_names")
        != 0;
    let nametags = client_world
        .stream
        .as_ref()
        .zip(
            cameras
                .single()
                .ok()
                .map(|(camera, transform)| (camera, GlobalTransform::from(*transform))),
        )
        .map(|(stream, (camera, transform))| {
            nametags::project_nametags(
                runtime.scoreboards(),
                stream,
                camera,
                &transform,
                [logical_width, logical_height],
                picked_nametag_actor(stream, &transform, collisions.as_deref()),
                actor_partial.0,
                show_names,
            )
        })
        .unwrap_or_default();
    presentation.set_nametag_anchors(nametags);
    presentation.set_chat_settings_snapshot(menu_runtime.settings_snapshot());
    let menu_view = menu_runtime.is_visible().then(|| {
        let mut view = menu_runtime.view();
        presentation.sync_menu_artwork(
            client_ui::ui_runtime::presentation::menu_artwork::view_paths(&view),
        );
        for server in view.featured.iter_mut().chain(view.gatherings.iter_mut()) {
            server.icon = presentation.menu_artwork_icon(&server.image_path);
        }
        view.featured_icon = presentation.item_icon("minecraft:compass_item", 0);
        view.gathering_icon = presentation.item_icon("minecraft:map_empty", 0);
        view.realm_icon = presentation.item_icon("minecraft:ender_pearl", 0);
        view.friend_icon = presentation.item_icon("minecraft:heart_of_the_sea", 0);
        view.saved_icon = presentation.item_icon("minecraft:book_normal", 0);
        view.profile_icon = presentation.player_preview_icon();
        view
    });
    presentation.set_menu_view(menu_view);
    presentation
        .refresh_scoreboard_owner_names(runtime.scoreboards(), client_world.stream.as_ref());
    presentation.publish_scene_inputs(&mut runtime);
    prepared.0 = Some(PendingUiPublication {
        inventory: runtime.capture_presentation_inventory(&player_runtime),
        preview,
        item_icons,
        now_millis,
        physical_size,
        dpi_scale,
    });
}

/// Captures predicted local movement and authoritative armor without server echo latency.
fn observe_paper_doll(
    stream: &chunk_pipeline::WorldStream,
    runtime: &client_ui::ui_runtime::UiRuntime,
    player_runtime: &crate::player_runtime::PlayerRuntime,
    physics: &crate::movement::LocalPhysicsController,
) -> Option<client_ui::ui_runtime::presentation::paper_doll::State> {
    use client_ui::ui_runtime::inventory_ledger::InventoryTarget;
    let actor = stream.authority().actor(stream.local_player_runtime_id())?;
    let flag = |bit: u32| {
        let key = if bit < 64 { 0 } else { 92 };
        match actor.metadata.get(&key) {
            Some(
                protocol::ActorMetadataValue::Flags(bits)
                | protocol::ActorMetadataValue::FlagsExtended(bits),
            ) => bits & (1 << (bit % 64)) != 0,
            _ => false,
        }
    };
    let (sneaking, sprinting) = physics.latest_sneak_sprint().unwrap_or((flag(1), flag(3)));
    Some(client_ui::ui_runtime::presentation::paper_doll::State {
        sneaking,
        sprinting,
        in_water: physics.in_water(),
        swimming: physics.mode() == sim::MovementMode::Swimming || flag(57),
        crawling: flag(114),
        flying: physics.mode() == sim::MovementMode::Flying,
        gliding: physics.mode() == sim::MovementMode::Gliding || flag(32),
        emoting: flag(92),
        armor: std::array::from_fn(|slot| {
            runtime
                .inventory_ledger(player_runtime)
                .target_stack(InventoryTarget::Armor(slot as u8))
                .map_or(0, |stack| stack.network_id)
        }),
    })
}

/// Resolves the crafter block's triggered state at the existing UI capture boundary.
fn station_triggered(
    stream: &chunk_pipeline::WorldStream,
    collisions: &crate::movement::PhysicsCollisionRegistries,
    position: [i32; 3],
) -> Option<bool> {
    let mode = stream.network_id_mode();
    let world = sim::PaletteWorld::new(
        stream.collision_store(),
        collisions.registry(mode),
        stream.current_dimension(),
    );
    let runtime_id = world.primary_runtime_id(position).ok()?;
    client_ui::ui_runtime::presentation::forms::container_data::state_bit(
        collisions.block_canonical_state(mode, runtime_id)?,
        "triggered_bit",
    )
}

/// Captures nametag picking with gameplay's reach and collision policy before UI projection.
fn picked_nametag_actor(
    stream: &chunk_pipeline::WorldStream,
    camera_transform: &GlobalTransform,
    collisions: Option<&crate::movement::PhysicsCollisionRegistries>,
) -> Option<u64> {
    let eye = camera_transform.translation();
    let direction = *camera_transform.forward();
    crate::melee::pick_actor(
        stream.authority().remote_actors(),
        None,
        eye.to_array(),
        direction.to_array(),
        crate::mining::survival_reach(protocol::PlayerInputMode::Mouse),
    )
    .filter(|hit| {
        let Some(collisions) = collisions else {
            return true;
        };
        let world = sim::PaletteWorld::new(
            stream.collision_store(),
            collisions.registry(stream.network_id_mode()),
            stream.current_dimension(),
        );
        let vector =
            |v: bevy::math::Vec3| sim::Vec3::new(f64::from(v.x), f64::from(v.y), f64::from(v.z));
        !matches!(
            world.block_interaction_ray_current(vector(eye), vector(direction), hit.distance),
            Ok(Some(_))
        )
    })
    .map(|hit| hit.runtime_id)
}

#[cfg(test)]
mod item_icons;
