//! Per-frame HUD observation and publication.

use super::*;

pub(crate) fn observe_mount_jump_input(
    input: Res<crate::semantic_controls::SemanticInputSnapshot>,
    mut runtime: ResMut<UiRuntime>,
    mut presentation: ResMut<UiPresentationRuntime>,
    time: Res<Time<Real>>,
) {
    let now_millis = u64::try_from(time.elapsed().as_millis()).unwrap_or(u64::MAX);
    runtime.set_mount_jump_held(input.phase(semantic_input::Action::Jump).held, now_millis);
    presentation.hud_frame_mut().tab_list_open =
        input.phase(semantic_input::Action::PlayerList).held;
}

pub(crate) fn platform_safe_area_insets() -> SafeArea {
    SafeArea::ZERO
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn publish_ui_runtime(
    mut runtime: ResMut<UiRuntime>,
    mut presentation: ResMut<UiPresentationRuntime>,
    mut scene: ResMut<UiRenderScene>,
    stats: Res<UiRenderStats>,
    visibility: Res<CaveVisibilityCache>,
    mut diagnostics_input: ResMut<VisibilityDiagnosticsInput>,
    visibility_diagnostics: Res<VisibilityDiagnostics>,
    render_queue: Res<ChunkRenderQueue>,
    upload_acknowledgements: Res<ChunkUploadAcknowledgements>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut client_world: ResMut<ClientWorld>,
    camera_settings: Res<CameraSettingsAuthority>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    frame_poll: Res<WorldStreamFramePoll>,
    time: Res<Time<Real>>,
    menu_runtime: Res<crate::menu::MenuRuntime>,
) {
    let Ok(window) = windows.single() else { return };
    let physical_size = [window.physical_width(), window.physical_height()];
    if physical_size.contains(&0) {
        return;
    }
    let logical_width = physical_size[0] as f32 / window.scale_factor();
    let logical_height = physical_size[1] as f32 / window.scale_factor();
    let Ok(dpi_scale) = DpiScale::new(window.scale_factor()) else {
        record_fatal_error(
            &mut client_world.fatal_error,
            "primary window reported an unsupported UI DPI scale".to_owned(),
        );
        return;
    };
    let now_millis = u64::try_from(time.elapsed().as_millis()).unwrap_or(u64::MAX);
    runtime.hud.expire(now_millis);
    if menu_runtime.is_visible() {
        presentation.set_loading_message(None);
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
        let startup_released = presentation.startup.observe(StartupReadinessInput {
            session_generation: runtime.session_id(),
            connected,
            diagnostics_frame_generation: diagnostics_input.frame_generation(),
            snapshot: visibility_diagnostics.snapshot(),
            visible_rendered: visibility.visible_rendered,
            cohort_target_complete: frame_poll
                .cohort
                .is_some_and(|status| status.target_is_complete()),
            stream_work_drained,
            render_work_drained,
        });
        diagnostics_input.set_startup_probe_enabled(presentation.startup.probe_enabled(connected));
        presentation.set_loading_message(if !connected {
            Some("Connecting to server...")
        } else if startup_released {
            None
        } else {
            Some("Loading terrain...")
        });
    }
    runtime.expire_gameplay_effects(now_millis);
    let skin = client_world.stream.as_ref().and_then(|stream| {
        let profile = stream.actor_player_profile(stream.local_player_runtime_id())?;
        let protocol::PlayerSkin::Standard(skin) = &profile.skin else {
            return None;
        };
        normalize_actor_skin(&ActorSkinPixels {
            width: skin.width,
            height: skin.height,
            rgba8: Arc::clone(&skin.rgba8),
        })
    });
    let pose = client_world
        .stream
        .as_ref()
        .and_then(|stream| stream.actor(stream.local_player_runtime_id()))
        .map_or_else(player_preview::PlayerPreviewPose::default, |actor| {
            let sneaking = matches!(
                actor.metadata.get(&0),
                Some(protocol::ActorMetadataValue::Flags(flags)) if flags & (1_u64 << 1) != 0
            );
            player_preview::PlayerPreviewPose::new(
                actor.body_yaw,
                actor.head_yaw,
                actor.pitch,
                sneaking,
            )
        });
    presentation.set_player_preview_skin(skin.as_deref(), pose);
    refresh_hud_frame(
        &mut runtime,
        &mut presentation,
        client_world.stream.as_ref(),
        &camera_settings,
        now_millis,
    );
    let anchors = client_world
        .stream
        .as_ref()
        .zip(cameras.single().ok())
        .map(|(stream, (camera, transform))| {
            project_below_name_anchors(
                runtime.scoreboards(),
                stream,
                camera,
                transform,
                [logical_width, logical_height],
                presentation.safe_area,
            )
        })
        .unwrap_or_default();
    presentation.set_below_name_anchors(anchors);
    let menu_view = menu_runtime.is_visible().then(|| {
        let mut view = menu_runtime.view();
        let artwork_paths = view
            .featured
            .iter()
            .chain(view.gatherings.iter())
            .filter(|server| !server.image_path.is_empty())
            .map(|server| server.image_path.clone())
            .collect();
        presentation.sync_menu_artwork(artwork_paths);
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
    if presentation.scoreboard_opacity.is_some() {
        presentation
            .refresh_scoreboard_owner_names(runtime.scoreboards(), client_world.stream.as_ref());
    }
    let input = match presentation.build(&runtime, now_millis, physical_size, dpi_scale) {
        Ok(input) => input,
        Err(error) => {
            record_fatal_error(&mut client_world.fatal_error, error.to_string());
            return;
        }
    };
    if let Err(error) = scene.publish(input, &stats) {
        record_fatal_error(
            &mut client_world.fatal_error,
            UiPresentationError::Render(error).to_string(),
        );
    }
}

pub(crate) fn refresh_hud_frame(
    runtime: &mut UiRuntime,
    presentation: &mut UiPresentationRuntime,
    stream: Option<&client_world::WorldStream>,
    camera_settings: &CameraSettingsAuthority,
    now_millis: u64,
) {
    let resolve_identifier = |stack: &protocol::NetworkItemStack| {
        stream.and_then(|stream| stream.canonical_item_stack(stack)?.identifier)
    };
    let derived_armor = runtime.gameplay_hud().armor().map(|slots| {
        let identifiers = [
            &slots.helmet,
            &slots.chestplate,
            &slots.leggings,
            &slots.boots,
        ]
        .map(|stack| {
            (!stack.is_empty())
                .then(|| resolve_identifier(stack))
                .flatten()
        });
        item_facts::total_armor_points(identifiers.iter().map(|id| id.as_deref()))
    });
    runtime.set_derived_armor(derived_armor);
    let mount_health = runtime
        .gameplay_hud()
        .mount_unique_id()
        .and_then(|unique| stream.and_then(|stream| stream.actor_health_by_unique(unique)));
    let mut hotbar_durability = [None; 9];
    let mut hotbar_icons = [None; 9];
    let mut hotbar_stacks: [Option<protocol::NetworkItemStack>; 9] = Default::default();
    let mut inventory_icons = super::hud_layout::InventoryIcons::default();
    for (slot, icon) in inventory_icons.0.iter_mut().enumerate() {
        if let Some(stack) = runtime.inventory_ledger().displayed_stack(slot as u8) {
            *icon = resolve_identifier(stack)
                .as_deref()
                .and_then(|id| presentation.item_icon(id, stack.metadata));
        }
    }
    let mut storage_icons = super::hud_layout::StorageIcons::default();
    for (slot, icon) in storage_icons.0.iter_mut().enumerate() {
        if let Some(stack) = runtime.inventory_ledger().storage_stack(slot as u8) {
            *icon = resolve_identifier(stack)
                .as_deref()
                .and_then(|id| presentation.item_icon(id, stack.metadata));
        }
    }
    let cursor_icon = runtime.inventory_ledger().cursor_stack().and_then(|stack| {
        resolve_identifier(stack)
            .as_deref()
            .and_then(|id| presentation.item_icon(id, stack.metadata))
    });
    let selected_snapshot = runtime.selected_stack_snapshot();
    let selected_slot = selected_snapshot.map(|snapshot| snapshot.slot);
    let selected_stack = selected_snapshot.and_then(|snapshot| match snapshot.state {
        crate::ui_runtime::inventory_ledger::PlayerInventorySlot::Present(stack) => Some(stack),
        crate::ui_runtime::inventory_ledger::PlayerInventorySlot::Unknown
        | crate::ui_runtime::inventory_ledger::PlayerInventorySlot::Empty => None,
    });
    for (slot, durability) in hotbar_durability.iter_mut().enumerate() {
        let slot = slot as u8;
        let stack = if selected_slot == Some(slot) {
            selected_stack
        } else if runtime.gameplay_hud().hotbar_known() {
            runtime.gameplay_hud().hotbar_stack(slot)
        } else {
            None
        };
        if let Some(stack) = stack {
            let identifier = resolve_identifier(stack);
            *durability = item_facts::cell_durability_fraction(
                stack,
                identifier.as_deref(),
                runtime
                    .inventory_ledger()
                    .slot_overlay(slot)
                    .map(|overlay| overlay.durability_correction),
            );
            hotbar_icons[usize::from(slot)] = identifier
                .as_deref()
                .and_then(|id| presentation.item_icon(id, stack.metadata));
            hotbar_stacks[usize::from(slot)] = Some(stack.clone());
        }
    }
    let offhand_durability = runtime.gameplay_hud().offhand_stack().and_then(|stack| {
        let identifier = resolve_identifier(stack);
        item_facts::durability_fraction(stack, identifier.as_deref())
    });
    let offhand_icon = runtime.gameplay_hud().offhand_stack().and_then(|stack| {
        let identifier = resolve_identifier(stack);
        identifier
            .as_deref()
            .and_then(|id| presentation.item_icon(id, stack.metadata))
    });
    let armor_icons = runtime.gameplay_hud().armor().map_or([None; 4], |armor| {
        [
            &armor.helmet,
            &armor.chestplate,
            &armor.leggings,
            &armor.boots,
        ]
        .map(|stack| {
            resolve_identifier(stack)
                .as_deref()
                .and_then(|id| presentation.item_icon(id, stack.metadata))
        })
    });
    let held_item_icon = selected_stack.and_then(|stack| {
        resolve_identifier(stack)
            .as_deref()
            .and_then(|id| presentation.item_icon(id, stack.metadata))
    });
    presentation.set_item_viewmodels(held_item_icon, offhand_icon);
    let (held_viewmodel_icon, offhand_viewmodel_icon) = presentation.item_viewmodel_icons();
    let selected_item_name = runtime.selected_stack_custom_name().or_else(|| {
        selected_stack.and_then(|stack| {
            resolve_identifier(stack).map(|id| Arc::from(runtime.localized_item_name(&id)))
        })
    });
    let selected_identity = selected_stack.map(|stack| (stack.network_id, stack.metadata));
    let mount_jump = runtime.gameplay_hud().mount_unique_id().and_then(|unique| {
        stream
            .filter(|stream| {
                stream.actor_has_attribute_by_unique(unique, "minecraft:horse.jump_strength")
            })
            .map(|_| runtime.mount_jump_charge(now_millis))
    });
    let first_person =
        camera_settings.perspective() == semantic_input::PerspectiveMode::FirstPerson;
    let player_preview_icon = presentation.player_preview_icon();
    let (left_hand_icon, right_hand_icon) = presentation.player_hand_icons();
    runtime.observe_selected_item_identity_value(selected_identity, now_millis);
    let frame = presentation.hud_frame_mut();
    frame.first_person = first_person;
    frame.mount_health = mount_health;
    frame.hotbar_durability = hotbar_durability;
    frame.hotbar_stacks = hotbar_stacks;
    frame.offhand_durability = offhand_durability;
    frame.hotbar_icons = hotbar_icons;
    frame.inventory_icons = inventory_icons;
    frame.storage_icons = storage_icons;
    frame.cursor_icon = cursor_icon;
    frame.armor_icons = armor_icons;
    frame.offhand_icon = offhand_icon;
    frame.offhand_viewmodel_icon = offhand_viewmodel_icon;
    frame.held_item_icon = held_viewmodel_icon;
    frame.player_preview = player_preview_icon;
    frame.left_hand = left_hand_icon;
    frame.right_hand = right_hand_icon;
    frame.viewmodel_pitch_degrees = stream
        .and_then(|stream| stream.actor(stream.local_player_runtime_id()))
        .map_or(0.0, |actor| actor.pitch);
    frame.selected_item_name = selected_item_name;
    frame.mount_jump = mount_jump;
    frame.attack_indicator_charge = Some(1.0);
    let diagnostics = runtime.gameplay_hud().diagnostics();
    if diagnostics != presentation.last_hud_diagnostics {
        bevy::log::debug!(
            skipped_effect_actions = diagnostics.skipped_effect_actions,
            evicted_effects = diagnostics.evicted_effects,
            odd_metadata_values = diagnostics.odd_metadata_values,
            dropped_inventory_events = diagnostics.dropped_inventory_events,
            odd_attribute_values = diagnostics.odd_attribute_values,
            odd_hud_packets = diagnostics.odd_hud_packets,
            oversized_chat_rows = diagnostics.oversized_chat_rows,
            unknown_effect_ids = diagnostics.unknown_effect_ids,
            "gameplay HUD skipped odd remote data"
        );
        presentation.last_hud_diagnostics = diagnostics;
    }
}

fn project_below_name_anchors(
    scoreboards: &ui::ScoreboardStore,
    stream: &client_world::WorldStream,
    camera: &Camera,
    camera_transform: &GlobalTransform,
    logical_size: [f32; 2],
    safe_area: SafeArea,
) -> Vec<BelowNameAnchor> {
    let content_width = (logical_size[0] - safe_area.left() - safe_area.right()).max(0.0);
    let content_height = (logical_size[1] - safe_area.top() - safe_area.bottom()).max(0.0);
    stream
        .render_players()
        .into_iter()
        .filter_map(|(actor, _profile)| {
            let below_name = scoreboards
                .below_name_for_owner(&ui::ScoreOwner::Player(actor.unique_id))
                .or_else(|| {
                    scoreboards.below_name_for_owner(&ui::ScoreOwner::Entity(actor.unique_id))
                })?;
            let name = stream.actor_display_name(actor.unique_id)?;
            let position = Vec3::from_array(actor.position) + Vec3::Y * 2.35;
            let viewport = camera.world_to_viewport(camera_transform, position).ok()?;
            let x = viewport.x - safe_area.left();
            let y = viewport.y - safe_area.top();
            (x.is_finite()
                && y.is_finite()
                && x >= 0.0
                && x <= content_width
                && y >= 0.0
                && y <= content_height)
                .then_some(BelowNameAnchor {
                    x,
                    y,
                    name,
                    score: below_name.0,
                    objective: below_name.1,
                })
        })
        .take(retained_hud::MAX_PRESENTED_BELOW_NAME_ROWS)
        .collect()
}
