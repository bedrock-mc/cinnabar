use std::sync::Arc;

use bevy::{
    camera::Camera,
    math::Vec3,
    prelude::{Camera3d, GlobalTransform, Query, Res, ResMut, With},
    time::Real,
    window::{PrimaryWindow, Window},
};
use render::{ActorSkinPixels, UiRenderScene, UiRenderStats, normalize_actor_skin};
use ui::{DpiScale, SafeArea};

use crate::{
    camera::CameraSettingsAuthority,
    runtime::{shutdown::record_fatal_error, world::ClientWorld},
    ui_runtime::{UiRuntime, item_facts},
};

use super::{
    UiPresentationError, UiPresentationRuntime,
    retained_hud::{self, BelowNameAnchor},
};

#[allow(clippy::too_many_arguments)]
pub(crate) fn publish_ui_runtime(
    mut runtime: ResMut<UiRuntime>,
    mut presentation: ResMut<UiPresentationRuntime>,
    mut scene: ResMut<UiRenderScene>,
    stats: Res<UiRenderStats>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut client_world: ResMut<ClientWorld>,
    menu_runtime: Res<crate::menu::MenuRuntime>,
    camera_settings: Res<CameraSettingsAuthority>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    time: Res<bevy::time::Time<Real>>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
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
    runtime.drain_pending_inventory();
    runtime.expire_gameplay_effects(now_millis);
    runtime.observe_selected_item_identity(now_millis);
    let player_preview_skin = client_world.stream.as_ref().and_then(|stream| {
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
    presentation.set_player_preview_skin(player_preview_skin.as_deref());
    refresh_hud_frame(
        &mut runtime,
        &mut presentation,
        client_world.stream.as_ref(),
        &camera_settings,
        now_millis,
    );
    let below_name_anchors = client_world
        .stream
        .as_ref()
        .zip(cameras.single().ok())
        .map(|(stream, (camera, camera_transform))| {
            project_below_name_anchors(
                runtime.scoreboards(),
                stream,
                camera,
                camera_transform,
                [logical_width, logical_height],
                presentation.safe_area,
            )
        })
        .unwrap_or_default();
    presentation.set_below_name_anchors(below_name_anchors);
    if presentation.scoreboard_opacity.is_some() {
        presentation
            .refresh_scoreboard_owner_names(runtime.scoreboards(), client_world.stream.as_ref());
    }
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

/// Refreshes the per-frame HUD inputs that need the world stream: derived
/// armor points, per-slot durability, the selected-item name, and the mount's
/// authoritative health. Without a stream every derived value fails closed.
fn refresh_hud_frame(
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
        item_facts::total_armor_points(identifiers.iter().map(|identifier| identifier.as_deref()))
    });
    runtime.set_derived_armor(derived_armor);

    let mount_health = runtime
        .gameplay_hud()
        .mount_unique_id()
        .and_then(|unique| stream.and_then(|stream| stream.actor_health_by_unique(unique)));

    let mut hotbar_durability = [None; 9];
    let mut hotbar_icons = [None; 9];
    for (slot, durability) in hotbar_durability.iter_mut().enumerate() {
        if let Some(stack) = runtime.presented_hotbar_stack(slot as u8) {
            let identifier = resolve_identifier(stack);
            *durability = item_facts::durability_fraction(stack, identifier.as_deref());
            hotbar_icons[slot] = identifier
                .as_deref()
                .and_then(|identifier| presentation.item_icon(identifier, stack.metadata));
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
            .and_then(|identifier| presentation.item_icon(identifier, stack.metadata))
    });
    let held_item_icon = runtime.selected_stack().and_then(|stack| {
        resolve_identifier(stack)
            .as_deref()
            .and_then(|identifier| presentation.item_icon(identifier, stack.metadata))
    });
    let selected_item_name = runtime.selected_stack().and_then(|stack| {
        resolve_identifier(stack)
            .map(|identifier| Arc::from(runtime.localized_item_name(&identifier)))
    });

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
    let frame = presentation.hud_frame_mut();
    frame.first_person = first_person;
    frame.mount_health = mount_health;
    frame.hotbar_durability = hotbar_durability;
    frame.offhand_durability = offhand_durability;
    frame.hotbar_icons = hotbar_icons;
    frame.offhand_icon = offhand_icon;
    frame.held_item_icon = held_item_icon;
    frame.player_preview = player_preview_icon;
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
