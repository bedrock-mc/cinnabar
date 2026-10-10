//! Per-frame HUD observation and publication.
use bevy::prelude::Transform;
use inventory::inventory_ledger::PlayerInventorySlot;
use {super::*, client_presentation::camera::CameraSettingsAuthority};

#[cfg(test)]
mod camera_hand_tests;
mod commit;
mod cooldowns;
mod loading;
use client_ui::ui_runtime::presentation::{
    ItemIconFrames, PendingUiPublication, PreparedUiPublication, PreviewCapture, capture_hud_frame,
    nametags, player_preview,
};
pub(crate) use commit::publish_ui_runtime;

pub(crate) fn observe_mount_jump_input(
    input: Res<crate::semantic_controls::SemanticInputSnapshot>,
    mut runtime: ResMut<UiRuntime>,
    time: Res<Time<Real>>,
) {
    let now_millis = u64::try_from(time.elapsed().as_millis()).unwrap_or(u64::MAX);
    runtime.set_mount_jump_held(input.phase(semantic_input::Action::Jump).held, now_millis);
    runtime.set_player_list_held(input.phase(semantic_input::Action::PlayerList).held);
}

pub(crate) fn platform_safe_area_insets() -> SafeArea {
    SafeArea::ZERO
}

/// CPU hand carriers follow the same camera capability as the animated hand rig.
fn hand_first_person(
    perspective: semantic_input::PerspectiveMode,
    server: Option<&client_presentation::camera::ServerCameraView>,
) -> bool {
    let fallback = perspective == semantic_input::PerspectiveMode::FirstPerson;
    server.map_or(fallback, |camera| camera.renders_first_person(fallback))
}

/// Resources beyond Bevy's sixteen-parameter limit.
type PublishExtras<'w> = (
    Res<'w, WorldStreamFramePoll>,
    Res<'w, crate::menu::MenuRuntime>,
    Res<'w, client_presentation::actor_publication::ActorFrameState>,
    Option<Res<'w, crate::movement::PhysicsCollisionRegistries>>,
    Option<Res<'w, render::RuntimeStageProfiler>>,
    Option<Res<'w, render::ActorPipelineReadiness>>,
    Option<Res<'w, render::PipelineWarmupReadiness>>,
    Option<Res<'w, client_presentation::camera::ServerCameraView>>,
    (
        Res<'w, client_presentation::actor_publication::ActorFramePartialTick>,
        Res<'w, client_presentation::local_player::LocalPlayerFrameCarrier>,
        Res<'w, crate::environment::WorldClock>,
        Res<'w, crate::environment::WeatherState>,
        Res<'w, crate::runtime::network::NetworkHandle>,
        Option<ResMut<'w, render::UiGlintSettings>>,
        Res<'w, crate::item_use::ItemUseRuntime>,
        Res<'w, crate::movement::MovementTicker>,
        Res<'w, crate::movement::LocalPhysicsController>,
    ),
);

/// Observes UI authority and captures inventory, including this frame's outbound predictions.
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
        actor_pipelines,
        warmed_pipelines,
        server_camera,
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
    let dpi_scale = match DpiScale::new(window.scale_factor()) {
        Ok(scale) => scale,
        Err(error) => {
            hand.clear();
            presentation.record_frame_failure(&UiPresentationError::Geometry(error));
            return;
        }
    };
    let now_millis = u64::try_from(time.elapsed().as_millis()).unwrap_or(u64::MAX);
    runtime.expire_hud(now_millis);
    let restart = client_world.dimension_transfer.take_presentation_reset();
    loading::prepare_loading(
        &mut client_world,
        &mut presentation,
        &runtime,
        &mut diagnostics_input,
        &network,
        loading::LoadingObservation {
            restart,
            menu_visible: menu_runtime.is_visible(),
            snapshot: visibility_diagnostics.snapshot(),
            visible_rendered: visibility.visible_rendered,
            cohort: frame_poll.cohort_progress,
            render_work_drained: render_queue.retained_len() == 0
                && upload_acknowledgements.is_empty(),
            pipelines_ready: actor_pipelines
                .as_ref()
                .is_none_or(|ready| ready.is_ready())
                && warmed_pipelines
                    .as_ref()
                    .is_none_or(|ready| ready.is_ready()),
            now: time.elapsed(),
        },
    );
    runtime.expire_gameplay_effects(now_millis);
    let stream = client_world.stream.as_ref();
    let menu_skin = menu_runtime.player_skin();
    let preview_ready = presentation.set_menu_preview_skin(&menu_skin.standard_skin());
    let own_pixels = render_model::ActorSkinPixels {
        width: menu_skin.width,
        height: menu_skin.height,
        rgba8: menu_skin.rgba8.clone(),
    };
    let skin = if menu_runtime.is_visible() {
        player_preview::local_preview_skin(None, &own_pixels)
    } else {
        player_preview::local_preview_skin(stream, &own_pixels)
    };
    let dressing_room = menu_runtime.is_visible()
        && menu_runtime.screen() == launcher::menu::MenuScreen::DressingRoom;
    let pose = if dressing_room {
        player_preview::PlayerPreviewPose::default()
    } else {
        player_preview::PlayerPreviewPose::of_local_player(stream)
    };
    presentation.set_preview_pack_equipment(
        client_world
            .pack_entities
            .as_ref()
            .and_then(|pack| pack.equipment.clone()),
    );
    // The model wears the local player's armor and held item.
    if dressing_room {
        presentation.set_player_preview_gear([None; 4], None);
    } else {
        presentation.dress_player_preview(&player_runtime, &runtime, |stack| {
            client_world
                .stream
                .as_ref()?
                .authority()
                .canonical_item_stack(stack)?
                .identifier
        });
    }
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
    let emote_preview = runtime
        .emotes()
        .playback()
        .map(|playback| (playback.emote, playback.elapsed(now_millis)))
        .or_else(|| {
            runtime
                .emotes()
                .is_open()
                .then(|| {
                    let slots = runtime.emotes().slots();
                    let selected = runtime
                        .emotes()
                        .selected_slot()
                        .and_then(|slot| slots[slot])
                        .or_else(|| slots.iter().copied().flatten().next())?;
                    Some((selected, now_millis as f64 / 1_000.0))
                })
                .flatten()
        });
    if hud_doll || runtime.emotes().is_open() {
        presentation.capture_hud_player_with_emote(
            client_world.stream.as_ref(),
            doll_state.is_some_and(|state| state.swimming),
            emote_preview,
        );
    }
    let overlays = client_presentation::presentation::visibility::GameplayOverlayVisibility::new(
        settings.value("hide_hud") != 0,
        settings.value("hide_hand") != 0,
    );
    let hide_hand = !overlays.hand;
    // The paper doll shows in the inventory and menus; the CPU hands only while no GPU hand rig.
    let first_person = hand_first_person(camera_settings.perspective(), server_camera.as_deref());
    let java_held_item = camera_settings.feel().java_animations
        && stream
            .and_then(|stream| {
                stream
                    .authority()
                    .actor_rig(stream.local_player_runtime_id())
            })
            .map_or_else(
                || {
                    player_runtime
                        .selected_stack_snapshot()
                        .is_some_and(|selected| {
                            matches!(selected.state, PlayerInventorySlot::Present(_))
                        })
                },
                |rig| rig.java_equipped.is_some(),
            );
    let preview = PreviewCapture {
        ready: preview_ready,
        skin,
        pose,
        shown: runtime.inventory_open()
            || menu_runtime.is_visible()
            || hud_doll
            || runtime.emotes().is_open(),
        hands: first_person && !hide_hand && !hand_rig.hand_is_active() && !java_held_item,
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
    presentation.hud_frame_mut().hotbar_cooldowns = cooldowns::hotbar_cooldowns(
        &player_runtime,
        &runtime,
        client_world.stream.as_ref(),
        &item_use,
        movement.completed_tick(),
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
    presentation.hud_frame_mut().first_person = first_person && !hide_hand;
    presentation.hud_frame_mut().hand_rig_active = hand_rig.hand_is_active();
    if hand_rig.hand_is_active() {
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
        .filter(|_| overlays.nametags)
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
        presentation.sync_menu_artwork_view(&view);
        for server in view.featured.iter_mut() {
            server.icon = presentation.menu_artwork_icon(&server.image_path);
        }
        view.featured_icon = presentation.item_icon("minecraft:compass_item", 0);
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
    use inventory::inventory_ledger::InventoryTarget;
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
        emoting: flag(92) || runtime.emotes().playback().is_some(),
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
    gameplay::melee::pick_actor(
        stream.authority().remote_actors(),
        None,
        eye.to_array(),
        direction.to_array(),
        gameplay::mining::survival_reach(protocol::PlayerInputMode::Mouse),
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
