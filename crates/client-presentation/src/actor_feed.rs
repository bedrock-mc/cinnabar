//! Converts borrowed local physics and skin observations into the actor animation feed.
use client_world::{LocalItemUse, LocalPlayerFeed};
/// Builds this frame's client-authored local-player feed from the predicted physics state and
/// the look pose. The yaw/pitch come from the look input (`LocalViewPose`), never the boomed
/// third-person camera. Returns `None` before physics or on any non-finite value.
pub fn build_local_player_feed(
    physics: &dyn crate::observations::PhysicsObservation,
    look: bevy::math::Quat,
    first_person: bool,
    view_bobbing: bool,
    local_uuid: [u8; 16],
    skin: impl FnOnce() -> protocol::PlayerSkin,
    item_use: LocalItemUse,
) -> Option<LocalPlayerFeed> {
    let state = physics.state()?;
    let (yaw, pitch, _) = look.to_euler(bevy::math::EulerRot::YXZ);
    let yaw_degrees = (180.0 - yaw.to_degrees()).rem_euclid(360.0);
    let pitch_degrees = -pitch.to_degrees();
    let position = [
        state.position.x as f32,
        state.position.y as f32,
        state.position.z as f32,
    ];
    let velocity = [
        state.velocity.x as f32,
        state.velocity.y as f32,
        state.velocity.z as f32,
    ];
    if !position
        .iter()
        .chain(&velocity)
        .chain(&[yaw_degrees, pitch_degrees])
        .all(|value| value.is_finite())
    {
        return None;
    }
    let (sneaking, sprinting) = physics.latest_sneak_sprint().unwrap_or_default();
    Some(LocalPlayerFeed {
        game_mode: None,
        prefer_client_skin: false,
        // A real player-list echo overrides this; without one, the stream backs the local body
        // with the client's own uploaded skin under this stable local uuid.
        uuid: local_uuid,
        username: std::sync::Arc::from(""),
        skin: skin(),
        position,
        velocity,
        on_ground: state.on_ground,
        yaw: yaw_degrees,
        head_yaw: yaw_degrees,
        pitch: pitch_degrees,
        main_hand: None,
        off_hand: None,
        held_network_ids: [0; 2],
        main_hand_metadata: 0,
        main_hand_slot: 0,
        main_hand_stack_id: None,
        bedrock_swing_ticks: actor_animation::ACTOR_SWING_TICKS,
        java_swing_ticks: actor_animation::ACTOR_SWING_TICKS,
        flying: matches!(physics.mode(), sim::MovementMode::Flying),
        gliding: matches!(physics.mode(), sim::MovementMode::Gliding),
        fall_fly_ticks: physics.fall_fly_ticks(),
        teleported: false,
        first_person,
        view_bobbing,
        sneaking,
        sprinting,
        item_use,
    })
}

/// Builds the local actor feed with selected equipment and admitted gameplay timing.
#[allow(clippy::too_many_arguments)]
pub fn prepare_local_player_feed(
    physics: &dyn crate::observations::PhysicsObservation,
    view: &crate::local_player::LocalViewPose,
    settings: &crate::camera::CameraSettingsAuthority,
    equipment: &crate::presentation::equipment::ActorEquipmentInput,
    player: &player_state::PlayerState,
    use_held: bool,
    local_use: impl FnOnce() -> LocalItemUse,
    java_blocking: &mut bool,
    local_uuid: [u8; 16],
    skin: impl FnOnce() -> protocol::PlayerSkin,
    prefer_client_skin: bool,
    swing_ticks: (i32, i32),
) -> Option<LocalPlayerFeed> {
    // Java blocks with a sword while use is held; Bedrock never flags that use.
    let java_sword = settings.feel().java_animations
        && equipment
            .main
            .as_ref()
            .is_some_and(|item| render_model::java_animation::is_java_sword(&item.identifier));
    let blocking = java_sword && use_held;
    let local_use = if blocking {
        client_world::LocalItemUse::Using
    } else {
        local_use()
    };
    // Ending a block clears the use flag it raised, whatever the hand holds next.
    let local_use =
        if (java_sword || *java_blocking) && local_use == client_world::LocalItemUse::Unpredicted {
            client_world::LocalItemUse::Idle
        } else {
            local_use
        };
    *java_blocking = blocking;
    let mut local_feed = build_local_player_feed(
        physics,
        view.rotation(),
        false,
        settings.feel().view_bobbing,
        local_uuid,
        skin,
        local_use,
    );
    if let Some(feed) = &mut local_feed {
        feed.game_mode = player.facts.player_game_mode();
        feed.prefer_client_skin = prefer_client_skin;
        feed.held_network_ids = [
            equipment
                .main
                .as_ref()
                .and_then(|_| player.selected_stack())
                .map_or(0, |stack| stack.network_id),
            equipment
                .off
                .as_ref()
                .and_then(|_| {
                    player
                        .inventory
                        .ledger()
                        .target_stack(inventory::inventory_ledger::InventoryTarget::Offhand)
                })
                .map_or(0, |stack| stack.network_id),
        ];
        feed.main_hand_slot = player.selected_hotbar_slot().unwrap_or(0);
        feed.main_hand_stack_id = player
            .selected_stack()
            .map(|stack| stack.stack_network_id)
            .filter(|id| *id > 0);
        feed.bedrock_swing_ticks = swing_ticks.0;
        feed.java_swing_ticks = swing_ticks.1;
    }
    local_feed
}
