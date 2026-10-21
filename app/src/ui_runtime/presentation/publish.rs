//! Per-frame HUD observation and publication.
use std::sync::Arc;

use crate::runtime::network::resource_packs::PackApplication;
use bevy::prelude::{Local, Transform, Vec3};
use client_runtime::network::NetworkHandle;
use inventory::inventory_ledger::PlayerInventorySlot;
use {super::*, client_presentation::camera::CameraSettingsAuthority};

use launcher::menu::settings_options::SettingId;
#[cfg(test)]
mod camera_hand_tests;
mod commit;
mod cooldowns;
mod loading;
mod menu_icons;
use client_ui::ui_runtime::presentation::{
    ItemIconFrames, PendingUiPublication, PreviewCapture, capture_hud_frame, nametags,
    player_preview,
};
pub(crate) use commit::publish_ui_runtime;

/// The UI authority captured before input enqueue, rendered once by `publish_ui_runtime`.
#[derive(bevy::prelude::Resource, Default)]
pub(crate) struct PreparedUiPublication(pub(crate) Option<PendingUiPublication>);

pub(crate) fn observe_mount_jump_input(
    input: Res<client_state::SemanticInputSnapshot>,
    mut runtime: ResMut<UiRuntimeResource>,
    mut presentation: ResMut<UiPresentationResource>,
    time: Res<Time<Real>>,
) {
    let now_millis = u64::try_from(time.elapsed().as_millis()).unwrap_or(u64::MAX);
    runtime.set_mount_jump_held(input.phase(semantic_input::Action::Jump).held, now_millis);
    runtime.set_player_list_held(input.phase(semantic_input::Action::PlayerList).held);
    presentation.set_form_input_mode(input.snapshot().map_or(json_ui::InputMode::Mouse, |input| {
        match input.input_mode {
            semantic_input::InputMode::KeyboardMouse => json_ui::InputMode::Mouse,
            semantic_input::InputMode::GamePad => json_ui::InputMode::Gamepad,
            semantic_input::InputMode::Touch => json_ui::InputMode::Touch,
        }
    }));
}

pub(crate) fn platform_safe_area_insets() -> SafeArea {
    SafeArea::ZERO
}

/// Resources beyond Bevy's sixteen-parameter limit.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct PublishExtras<'w, 's> {
    frame_poll: Res<'w, WorldStreamFramePoll>,
    #[cfg(not(target_arch = "wasm32"))]
    menu_runtime: Res<'w, client_shell::menu::MenuRuntime>,
    #[cfg(target_arch = "wasm32")]
    options: Res<'w, crate::browser_input::BrowserOptions>,
    #[cfg(target_arch = "wasm32")]
    skin: Res<'w, client_shell::player_skin::LocalPlayerSkin>,
    hand_rig: Res<'w, client_presentation::actor_publication::ActorFrameState>,
    collisions: Option<Res<'w, client_interaction::movement::PhysicsCollisionRegistries>>,
    profiler: Option<Res<'w, render_scene::RuntimeStageProfiler>>,
    actor_pipelines: Option<Res<'w, render_scene::ActorPipelineReadiness>>,
    warmed_pipelines: Option<Res<'w, render_scene::PipelineWarmupReadiness>>,
    server_camera: Option<Res<'w, client_presentation::camera::ServerCameraView>>,
    nametag_cache: Local<'s, nametags::NametagTextCache>,
    fatal_error: ResMut<'w, crate::runtime::shutdown::FatalError>,
    actor_partial: Res<'w, client_presentation::actor_publication::ActorFramePartialTick>,
    local_frame: Res<'w, client_presentation::local_player::LocalPlayerFrameCarrier>,
    local_visibility: Res<'w, client_presentation::local_player::LocalAvatarVisibilityCarrier>,
    clock: Res<'w, client_presentation::environment::WorldClock>,
    weather: Res<'w, client_presentation::environment::WeatherState>,
    network: Res<'w, NetworkHandle<PackApplication>>,
    glint_settings: Option<ResMut<'w, render_scene::UiGlintSettings>>,
    item_use: Res<'w, client_interaction::item_use::ItemUseRuntime>,
    melee: Res<'w, client_interaction::melee::MeleeRuntime>,
    movement: Res<'w, client_interaction::movement::MovementTicker>,
    physics: Res<'w, client_interaction::movement::LocalPhysicsController>,
}

/// Observes UI authority and captures inventory, including this frame's outbound predictions.
#[allow(clippy::too_many_arguments)]
pub(crate) fn prepare_ui_runtime(
    player_runtime: bevy::prelude::Res<client_state::PlayerRuntime>,
    mut runtime: ResMut<UiRuntimeResource>,
    mut presentation: ResMut<UiPresentationResource>,
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
    extras: PublishExtras,
    mut hand: crate::presentation::viewmodel::ViewmodelPublish,
) {
    let PublishExtras {
        frame_poll,
        #[cfg(not(target_arch = "wasm32"))]
        menu_runtime,
        #[cfg(target_arch = "wasm32")]
        options,
        #[cfg(target_arch = "wasm32")]
        skin,
        hand_rig,
        collisions,
        profiler,
        actor_pipelines,
        warmed_pipelines,
        server_camera,
        mut nametag_cache,
        mut fatal_error,
        actor_partial,
        local_frame,
        local_visibility,
        clock,
        weather,
        network,
        glint_settings,
        item_use,
        melee,
        movement,
        physics,
    } = extras;
    #[cfg(not(target_arch = "wasm32"))]
    let (settings_snapshot, menu_skin, menu_visible, menu_scene, menu) = (
        menu_runtime.settings_snapshot(),
        menu_runtime.player_skin(),
        menu_runtime.is_visible(),
        Some(menu_runtime.screen()),
        Some(
            &*menu_runtime
                as &dyn client_ui::ui_runtime::presentation::forms::scene_policy::MenuScene,
        ),
    );
    #[cfg(target_arch = "wasm32")]
    let (settings_snapshot, menu_skin, menu_visible, menu_scene, menu) = (
        (Arc::clone(&options.0), None),
        &*skin,
        false,
        None::<launcher::menu::MenuScreen>,
        None,
    );
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(render_scene::RuntimeStage::UiPreparation));
    prepared.0 = None;
    let totem_style = presentation
        .mod_ui()
        .mod_actor_style()
        .and_then(|style| style.totems.as_ref())
        .cloned();
    if let Some(stream) = client_world.stream.as_mut() {
        stream.set_totem_tracking(totem_style.is_some());
        runtime.player_roster.set_totem_annotations(
            totem_style.as_ref(),
            stream.authority().player_totem_activations(),
        );
    } else {
        runtime
            .player_roster
            .set_totem_annotations(None, std::iter::empty());
    }
    let now_millis = u64::try_from(time.elapsed().as_millis()).unwrap_or(u64::MAX);
    runtime.advance_item_activation(now_millis);
    runtime.set_toast_lifetime(settings_snapshot.0.toast_lifetime_millis());
    if let Some(mut glint_settings) = glint_settings {
        #[cfg(not(target_arch = "wasm32"))]
        {
            *glint_settings = menu_runtime.ui_glint_settings();
        }
        #[cfg(target_arch = "wasm32")]
        {
            *glint_settings = render_scene::UiGlintSettings::default();
        }
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
    let dpi_scale = match DpiScale::new(window.scale_factor()) {
        Ok(scale) => scale,
        Err(error) => {
            hand.clear();
            presentation.record_frame_failure(&UiPresentationError::Geometry(error));
            return;
        }
    };
    runtime.expire_hud(now_millis);
    let restart = client_world.dimension_transfer.take_presentation_reset();
    loading::prepare_loading(
        &mut client_world,
        &mut fatal_error.0,
        &mut presentation,
        &runtime,
        &mut diagnostics_input,
        &network,
        loading::LoadingObservation {
            restart,
            menu_visible,
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
    presentation.set_preview_cosmetics(&menu_skin.cosmetics);
    presentation.set_cosmetic_motion(&hand_rig.cosmetic_motion());
    let preview_ready = presentation.set_menu_preview_skin(&menu_skin.standard_skin());
    let own_pixels = render_model::ActorSkinPixels {
        width: menu_skin.width,
        height: menu_skin.height,
        rgba8: menu_skin.rgba8.clone(),
    };
    let skin = if menu_visible {
        player_preview::local_preview_skin(None, &own_pixels)
    } else {
        player_preview::local_preview_skin(stream, &own_pixels)
    };
    let dressing_room =
        menu_visible && menu_scene == Some(launcher::menu::MenuScreen::DressingRoom);
    let pose = if dressing_room {
        player_preview::PlayerPreviewPose::default()
    } else {
        player_preview::PlayerPreviewPose::of_local_player(stream)
    };
    presentation.set_preview_equipment_assets(
        client_world.entity_assets.clone(),
        client_world
            .pack_entities
            .as_ref()
            .map(|pack| Arc::clone(&pack.assets)),
    );
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
        presentation.dress_player_preview(
            &player_runtime,
            &runtime,
            stream,
            actor_animation::AttachableAnimationInput {
                frame_alpha: actor_partial.0,
                delta_seconds: Some(time.delta_secs()),
                ..Default::default()
            },
            |stack| {
                client_world
                    .stream
                    .as_ref()?
                    .authority()
                    .canonical_item_stack(stack)
            },
        );
    }
    let doll_state = client_world
        .stream
        .as_ref()
        .and_then(|stream| observe_paper_doll(stream, &runtime, &player_runtime, &physics));
    presentation.observe_paper_doll(now_millis, doll_state);
    let settings = Arc::clone(&settings_snapshot.0);
    let hud_doll = presentation.hud_frame_mut().paper_doll_visible
        && settings.value(SettingId::HideHud) == 0
        && settings.value(SettingId::HidePaperdoll) == 0
        && !menu_visible
        && !runtime.inventory_open();
    // A visible control preview may need the local model while the gameplay doll stays hidden.
    let control_preview_player = presentation.control_preview_needs_player(runtime.session_id());
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
    if hud_doll || runtime.emotes().is_open() || control_preview_player {
        presentation.capture_hud_player_with_emote(
            client_world.stream.as_ref(),
            doll_state.is_some_and(|state| state.swimming),
            emote_preview,
        );
    }
    let overlays = client_presentation::presentation::visibility::GameplayOverlayVisibility::new(
        settings.value(SettingId::HideHud) != 0,
        settings.value(SettingId::HideHand) != 0,
        player_runtime.facts.player_game_mode() == Some(protocol::PlayerGameMode::Spectator),
    );
    let hide_hand = !overlays.hand;
    // The paper doll shows in the inventory and menus; the CPU hands only while no GPU hand rig.
    let first_person = client_presentation::camera::hand_first_person(
        camera_settings.perspective(),
        server_camera.as_deref(),
    );
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
            || menu_visible
            || hud_doll
            || runtime.emotes().is_open()
            || control_preview_player,
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
        &melee,
        movement.completed_tick(),
    );
    // Floored feet position and absolute world tick for the HUD's position and days-played text.
    presentation.hud_frame_mut().player_block = local_frame.snapshot().map(|frame| {
        let feet = frame.feet();
        [feet.x, feet.y, feet.z].map(|axis| axis.floor() as i32)
    });
    presentation.hud_frame_mut().thunderstorm = weather.lightning_level() > 0.0;
    presentation.hud_frame_mut().death_screen =
        menu_visible && menu_scene == Some(launcher::menu::MenuScreen::Death);
    presentation.hud_frame_mut().dimension = client_world
        .stream
        .as_ref()
        .map_or(0, |stream| stream.current_dimension());
    presentation.hud_frame_mut().world_time = Some(
        client_presentation::environment::visual_world_time(*clock, time.elapsed_secs_f64()),
    );
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
                || !presentation.renders_game_behind(&player_runtime, &runtime, menu)
                || presentation.loading_stage().is_some(),
            physical_size,
        );
    }
    runtime
        .player_roster
        .set_show_badges(settings.value(SettingId::CinnabarBadges) != 0);
    let show_names = settings.value(SettingId::IngamePlayerNames) != 0;
    let tag_style = presentation
        .mod_ui()
        .mod_actor_style()
        .and_then(|style| style.nametags.as_ref());
    let show_counter_self = totem_style
        .as_ref()
        .is_some_and(|style| style.show_self && style.show_nametag)
        && client_world.stream.as_ref().is_some_and(|stream| {
            let authority = stream.authority();
            authority
                .totem_activations(authority.local_player_runtime_id())
                .is_some_and(|count| count != 0)
        });
    let show_self_tag = (tag_style.is_some_and(|style| style.show_self) || show_counter_self)
        && !tag_style.is_some_and(|style| style.hide_players)
        && !first_person;
    let names_with_hidden_hud = tag_style.is_some_and(|style| style.show_when_hud_hidden);
    let nametags = client_world
        .stream
        .as_ref()
        .filter(|_| {
            overlays.nametags
                || (names_with_hidden_hud
                    && settings.value(SettingId::HideHud) != 0
                    && !menu_visible)
        })
        .zip(
            cameras
                .single()
                .ok()
                .map(|(_, transform)| GlobalTransform::from(*transform)),
        )
        .map(|(stream, transform)| {
            let authority = stream.authority();
            let viewer_position = local_frame
                .snapshot()
                .map(|frame| frame.feet())
                .or_else(|| {
                    authority
                        .actor(authority.local_player_runtime_id())
                        .and_then(|actor| actor.interpolated_position(actor_partial.0))
                        .map(Vec3::from_array)
                })
                .unwrap_or_else(|| transform.translation());
            let nametag_view = nametags::NametagView {
                viewer_position,
                camera_position: transform.translation(),
                camera_forward: *transform.forward(),
            };
            let mut tags = nametags::project_nametags(
                &mut nametag_cache,
                runtime.scoreboards(),
                stream,
                nametag_view,
                picked_nametag_actor(stream, &transform, collisions.as_deref(), &player_runtime),
                actor_partial.0,
                show_names && !tag_style.is_some_and(|style| style.hide_players),
            );
            if tag_style.is_some_and(|style| style.hide_passengers) {
                let authority = stream.authority();
                tags.retain(|tag| {
                    authority
                        .actor(tag.runtime_id())
                        .is_some_and(|actor| authority.ridden_unique_id(actor.unique_id).is_none())
                });
            }
            if show_self_tag && show_names {
                let authority = stream.authority();
                if let Some(visibility) = local_visibility.snapshot().filter(|visibility| {
                    visibility.visible()
                        && visibility.runtime_id() == authority.local_player_runtime_id()
                        && visibility.session_generation().get() == clock.session_generation()
                }) && let Some(actor) = authority
                    .actor(authority.local_player_runtime_id())
                    .filter(|actor| {
                        !tag_style.is_some_and(|style| style.hide_passengers)
                            || authority.ridden_unique_id(actor.unique_id).is_none()
                    })
                    && let Some(name) = authority
                        .actor_name_tag(actor.unique_id)
                        .or_else(|| authority.actor_display_name(actor.unique_id))
                    && let Some(tag) = nametags::extract_nametag_at_feet(
                        actor,
                        nametags::NametagView {
                            viewer_position: visibility.feet(),
                            ..nametag_view
                        },
                        None,
                        name,
                        runtime.scoreboards(),
                        visibility.feet(),
                    )
                {
                    tags.push(tag);
                }
            }
            for tag in &mut tags {
                tag.set_cinnabar_badge(
                    authority
                        .actor_player_roster_identity(tag.runtime_id())
                        .is_some_and(|(xuid, uuid)| runtime.player_roster.has_badge(xuid, uuid)),
                );
                if totem_style.as_ref().is_some_and(|style| style.show_nametag)
                    && let Some(actor) = authority.actor(tag.runtime_id())
                    && let protocol::ActorKind::Player { uuid, .. } = &actor.kind
                {
                    tag.set_counter_annotation(
                        runtime.player_roster.totem_annotation(uuid).cloned(),
                    );
                }
            }
            tags
        })
        .unwrap_or_default();
    presentation.set_nametag_local_actor(
        client_world
            .stream
            .as_ref()
            .map(|stream| stream.authority().local_player_runtime_id()),
    );
    presentation.set_nametag_anchors(nametags);
    presentation.set_chat_settings_snapshot(settings_snapshot);
    #[cfg(not(target_arch = "wasm32"))]
    let menu_view = menu_visible.then(|| {
        let mut view = menu_runtime.view();
        presentation.sync_menu_artwork_view(&view);
        for server in view.featured.iter_mut() {
            server.icon = presentation.menu_artwork_icon(&server.image_path);
        }
        menu_icons::install(&presentation, &mut view);
        view.profile_icon = presentation.player_preview_icon();
        view
    });
    #[cfg(target_arch = "wasm32")]
    let menu_view = None;
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
    player_runtime: &client_state::PlayerRuntime,
    physics: &client_interaction::movement::LocalPhysicsController,
) -> Option<client_ui::ui_runtime::presentation::paper_doll::State> {
    use inventory::inventory_ledger::InventoryTarget;
    let actor = stream.authority().actor(stream.local_player_runtime_id())?;
    use protocol_ids::ActorFlag;
    let flag = |flag: ActorFlag| actor_animation::AnimatedActor::flag(actor, flag.wire_value());
    let (sneaking, sprinting) = physics
        .latest_sneak_sprint()
        .unwrap_or((flag(ActorFlag::Sneaking), flag(ActorFlag::Sprinting)));
    Some(client_ui::ui_runtime::presentation::paper_doll::State {
        sneaking,
        sprinting,
        in_water: physics.in_water(),
        swimming: physics.mode() == sim::MovementMode::Swimming || flag(ActorFlag::Swimming),
        crawling: flag(ActorFlag::Crawling),
        flying: physics.mode() == sim::MovementMode::Flying,
        gliding: physics.mode() == sim::MovementMode::Gliding || flag(ActorFlag::Gliding),
        emoting: flag(ActorFlag::Emoting) || runtime.emotes().playback().is_some(),
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
    collisions: &client_interaction::movement::PhysicsCollisionRegistries,
    position: [i32; 3],
) -> Option<bool> {
    let mode = stream.network_id_mode();
    let world = sim::PaletteWorld::new(
        stream.collision_store(),
        collisions.registry(mode),
        stream.current_dimension(),
        Default::default(),
    );
    let runtime_id = world.primary_runtime_id(position).ok()?;
    assets::block_presentation_state::pinned_block_presentation_states()
        .get(mode, runtime_id)
        .crafter_triggered
}

/// Captures nametag picking with gameplay's reach and collision policy before UI projection.
fn picked_nametag_actor(
    stream: &chunk_pipeline::WorldStream,
    camera_transform: &GlobalTransform,
    collisions: Option<&client_interaction::movement::PhysicsCollisionRegistries>,
    player: &client_state::PlayerRuntime,
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
            crate::interaction_authority::pick_context(
                stream,
                collisions,
                player.facts.player_game_mode(),
                player.selected_stack().map(|item| item.block_runtime_id),
            ),
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
