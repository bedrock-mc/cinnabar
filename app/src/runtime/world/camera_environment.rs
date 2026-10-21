use bevy::prelude::{Query, Res, ResMut, Transform, With};
use client_presentation::{
    camera::FlyCamera,
    environment::{
        EnvironmentContext,
        fog::{FogBindings, fog_biome_samples, fog_precipitation_count},
    },
};
use meshing::CameraMedium;

use super::ClientWorld;
use crate::environment;

/// Samples the render camera's medium, biome and fog into the current environment.
pub(crate) fn update_camera_medium(
    client_world: Res<ClientWorld>,
    camera: Query<&Transform, With<FlyCamera>>,
    mut medium: ResMut<environment::CameraMediumState>,
    mut context: ResMut<EnvironmentContext>,
    mut fog_bindings: ResMut<FogBindings>,
    atmosphere: Res<render_scene::AtmosphereTextureAssets>,
    mut arena_hint: ResMut<render_scene::chunk::capacity::ChunkArenaCapacityHint>,
) {
    let Some((stream, camera)) = client_world.stream.as_ref().zip(camera.single().ok()) else {
        arena_hint.sections = 0;
        medium.0 = CameraMedium::Air;
        *context = EnvironmentContext::default();
        return;
    };
    *arena_hint = render_scene::chunk::capacity::ChunkArenaCapacityHint::for_view(
        (stream.render_distance_blocks() / 16.0) as usize,
        stream
            .dimension_range(stream.current_dimension())
            .map_or(0, |range| range.sub_chunk_count),
    );
    let position = camera.translation.to_array();
    fog_bindings.refresh(
        stream.authority().resolved_biome_tints(),
        stream.biome_tint_identity(),
        Some(&atmosphere),
    );
    let raw_id = stream.camera_biome_id(position);
    let (camera_profile, profile_route, camera_fog) =
        fog_bindings.camera(raw_id, stream.current_dimension());
    medium.0 = stream.camera_medium(position);
    let camera_biome =
        raw_id.and_then(|raw_id| stream.authority().resolved_biome_tints().biome(raw_id));
    *context = EnvironmentContext {
        dimension: stream.current_dimension(),
        fog_registry_generation: fog_bindings.atmosphere_generation,
        fog_biomes: Some(fog_biome_samples(stream, position, &fog_bindings)),
        camera_profile,
        camera_fog,
        profile_route,
        default_fog: fog_bindings.default_fog,
        precipitation_sample_count: fog_precipitation_count(stream, position),
        camera_biome_identifier: fog_bindings.identifier(raw_id),
        camera_biome_temperature: camera_biome.map(|biome| biome.temperature),
        render_distance_blocks: Some(stream.render_distance_blocks()),
    };
}
