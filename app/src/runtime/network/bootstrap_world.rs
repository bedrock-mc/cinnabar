use super::{
    bootstrap_timing,
    resource_packs::{self, PackApplication},
    server_animation,
};
use crate::runtime::world::ClientWorld;
use bevy::{log::warn, prelude::Mut};
use chunk_pipeline::WorldStream;
use client_runtime::publication::PublicationController;
use gameplay::movement::PhysicsCollisionRegistries;
use render_scene::{BlockEntitySceneResource, ChunkTextureAssets};
use std::{sync::Arc, time::Duration};

/// Server facts needed by the new stream, independent of UI and player authority.
pub(super) struct StreamBootstrap<'a> {
    pub world: protocol::WorldBootstrap,
    pub custom_blocks: &'a protocol::CustomBlocks,
    pub item_registry: Option<protocol::ItemRegistryEvent>,
    pub world_default_game_mode: protocol::GameModeUpdate,
    pub terrain_before_spawn: bool,
}

/// Collision and render owners the stream must share with the application.
pub(super) struct StreamResources<'a> {
    pub client_world: &'a mut ClientWorld,
    pub collisions: &'a mut PhysicsCollisionRegistries,
    pub chunk_textures: Option<Mut<'a, ChunkTextureAssets>>,
    pub block_entity_scene: Option<Mut<'a, BlockEntitySceneResource>>,
    pub publication: &'a PublicationController,
    pub display_interval: Duration,
}

/// Builds the session stream and installs its artwork before local authority can bind to it.
pub(super) fn build(
    input: StreamBootstrap<'_>,
    packs: &PackApplication,
    registries: super::session_registries::PreparedSessionRegistries,
    current: [f32; 3],
    resources: StreamResources<'_>,
    timings: &mut bootstrap_timing::BootstrapTimings,
) -> WorldStream {
    let StreamBootstrap {
        world: bootstrap,
        custom_blocks,
        item_registry,
        world_default_game_mode,
        terrain_before_spawn,
    } = input;
    let StreamResources {
        client_world,
        collisions,
        chunk_textures,
        block_entity_scene,
        publication,
        display_interval,
    } = resources;
    let hashed_ids = bootstrap.block_network_ids_are_hashes;
    let old = std::mem::replace(collisions, registries.collisions);
    client_runtime::session::release_off_frame(old);
    let old = std::mem::replace(&mut client_world.mining_filters, registries.mining_filters);
    client_runtime::session::release_off_frame(old);
    let registration = registries.registration;
    let custom_block_ids = registration.custom_ids;
    let id_remap = registration.remap;
    if !registration.supported {
        if hashed_ids {
            warn!("server custom blocks have no collision base in hashed id mode");
        } else {
            warn!(
                count = custom_blocks.blocks.len(),
                "server custom blocks are unsupported in this id ordering"
            );
        }
    }
    timings.mark(bootstrap_timing::BootstrapPhase::CustomBlocks);
    let session_assets = registries.assets;
    if let Some(mut textures) = chunk_textures {
        resource_packs::install_chunk_textures(&mut textures, &session_assets);
    }
    timings.mark(bootstrap_timing::BootstrapPhase::BlockAssets);
    let mut stream = if let Some(entity_assets) = client_world.entity_assets.as_ref() {
        WorldStream::new_with_asset_sets(
            bootstrap,
            Arc::clone(&session_assets),
            Arc::clone(entity_assets),
            current,
            client_world.pending_surface_spawn,
        )
    } else {
        WorldStream::new_with_assets(
            bootstrap,
            session_assets,
            current,
            client_world.pending_surface_spawn,
        )
    };
    if custom_blocks.skipped != 0 {
        warn!(
            skipped = custom_blocks.skipped,
            "skipped malformed server block definitions"
        );
    }
    if bridge::latency_trace::active().is_some() {
        stream.observe_commits(super::world_ingress::trace_committed);
    }
    stream.set_server_animation_compiler(server_animation::compile_stop);
    stream.set_world_default_game_mode(world_default_game_mode);
    stream.set_display_interval(display_interval);
    stream.begin_frame_work();
    stream.set_startup_priority(true);
    stream.set_startup_terrain_announced(terrain_before_spawn);
    stream.set_custom_block_ids(custom_block_ids.unwrap_or_default());
    stream.set_sequential_id_remap(id_remap);
    stream.install_custom_block_identities(registries.identities);
    stream.set_light_diagnostic_custom_blocks(custom_blocks.clone());
    stream.set_pack_entities(packs.entities.as_ref().map(|pack| {
        (
            Arc::clone(&pack.assets),
            pack.bindings
                .iter()
                .map(|binding| binding.geometry_candidate)
                .collect(),
        )
    }));
    stream.seed_property_defaults(&packs.property_defaults);
    if let (Some(mut scene), Some(art)) = (block_entity_scene, packs.block_entity_art.as_ref()) {
        scene.install_prepared_assets(art);
    }
    client_world.pack_entities = packs.entities.clone();
    client_world.terrain_resource_identity = packs.terrain_identity;
    client_world.prepared_actor_artwork = packs.prepared_actor_artwork.clone();
    client_world.session_items = Some(Arc::new(
        client_presentation::session_assets::SessionItems {
            components: packs.item_components.clone().unwrap_or_default(),
            icons: packs.item_icons.clone(),
        },
    ));
    let digger_filters = Arc::new(pack_runtime::DiggerFilters::new(
        item_registry
            .iter()
            .flat_map(|registry| registry.entries.iter()),
    ));
    let old = std::mem::replace(&mut client_world.digger_filters, digger_filters);
    client_runtime::session::release_off_frame(old);
    if let Some(registry) = item_registry
        && !stream.seed_item_registry(registry)
    {
        warn!("StartGame item registry was refused by the world stream");
    }
    stream.set_publication_allowance(publication.allowance());
    stream
}

/// Prepares tag programs for digger queries that a later item registry introduces.
pub(super) fn admit_item_registry_diggers(
    client_world: &mut ClientWorld,
    entries: &[protocol::ItemRegistryEntry],
) {
    if let Some(extended) = client_world.digger_filters.extended(entries) {
        let old = std::mem::replace(&mut client_world.digger_filters, Arc::new(extended));
        client_runtime::session::release_off_frame(old);
    }
}
