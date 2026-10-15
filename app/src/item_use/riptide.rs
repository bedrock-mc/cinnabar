//! World observations used when pressing and releasing a trident.

/// Captures the release facing after any action aim override has been applied.
pub(super) fn request_launch(
    level: u8,
    physics: &mut client_interaction::movement::LocalPhysicsController,
    movement: &mut client_interaction::movement::MovementTicker,
    view: &client_presentation::local_player::LocalViewPose,
) {
    let (yaw, pitch, _) = view.rotation().to_euler(bevy::prelude::EulerRot::YXZ);
    movement.request_riptide(
        physics,
        level,
        [
            -pitch.to_degrees(),
            gameplay::movement::wire_yaw(180.0 - yaw.to_degrees()),
        ],
    );
}

/// Tests rain exposure at the player's upper body, using the loaded weather column.
pub(super) fn in_rain(stream: &chunk_pipeline::WorldStream, position: [f32; 3], rain: f32) -> bool {
    if rain <= 0.2 || !rain.is_finite() || stream.current_dimension() != 0 {
        return false;
    }
    let point = position.map(f32::floor);
    let x = point[0] as i32;
    let z = point[2] as i32;
    let Some(top) = stream.top_non_air_block_y(x, z) else {
        return false;
    };
    if point[1] < top.saturating_add(1) as f32 {
        return false;
    }
    let Some(biome) = stream
        .camera_biome_id(point)
        .and_then(|id| stream.authority().resolved_biome_tints().biome(id))
    else {
        return false;
    };
    world::weather::classify_precipitation(biome.temperature, biome.downfall, point[1] as i32)
        == world::weather::Precipitation::Rain
}
