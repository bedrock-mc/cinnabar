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
    pub player_game_mode_uses_world_default: bool,
    pub terrain_before_spawn: bool,
}

/// Collision and render owners the stream must share with the application.
pub(super) struct StreamResources<'a> {
    pub client_world: &'a mut ClientWorld,
    pub collisions: &'a mut PhysicsCollisionRegistries,
    pub chunk_textures: Option<Mut<'a, ChunkTextureAssets>>,
    pub block_entity_scene: Option<Mut<'a, BlockEntitySceneResource>>,
    pub actor_artwork: Option<Mut<'a, render_scene::ActorArtworkResource>>,
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
        player_game_mode_uses_world_default,
        terrain_before_spawn,
    } = input;
    let StreamResources {
        client_world,
        collisions,
        chunk_textures,
        block_entity_scene,
        actor_artwork,
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
    stream.set_local_game_mode_follows_default(player_game_mode_uses_world_default);
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
    if let Some(mut artwork) = actor_artwork {
        install_prepared_artwork(
            &mut artwork.0,
            packs.prepared_actor_artwork.as_deref(),
            packs.entity_artwork.as_deref(),
        );
    }
    client_world.prepared_actor_artwork = packs.prepared_actor_artwork.clone();
    client_world.session_items = Some(Arc::new(
        client_presentation::session_assets::SessionItems {
            components: packs.item_components.clone().unwrap_or_default(),
            icons: packs.item_icons.clone(),
        },
    ));
    if let Some(registry) = item_registry
        && !stream.seed_item_registry(registry)
    {
        warn!("StartGame item registry was refused by the world stream");
    }
    stream.set_publication_allowance(publication.allowance());
    stream
}

/// Publishes the artwork the join prepared its actor resources over, which a reload during the
/// join may have replaced, so the first world frame reuses them instead of rebuilding them.
/// Only a base with the joined stack's own artwork identity is installed: one captured from a
/// retired server is never republished, and its prepared resources are rebuilt instead.
fn install_prepared_artwork(
    current: &mut render_model::ActorArtworkPages,
    prepared: Option<&client_presentation::prepared_actor_artwork::PreparedActorArtwork>,
    stack: Option<&render_model::ActorArtworkPages>,
) {
    if let (Some(prepared), Some(stack)) = (prepared, stack)
        && prepared.base().identity() == stack.identity()
        && !prepared.base().shares_storage_with(current)
    {
        *current = prepared.base().clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use client_presentation::prepared_actor_artwork::PreparedActorArtwork;
    use render_model::{ActorArtworkPages, EquipmentRaster};

    /// Startup pages with one stack texture, compiled into fresh storage on each call.
    fn stack_artwork(seed: u8) -> ActorArtworkPages {
        ActorArtworkPages::default()
            .with_equipment_rasters(&[EquipmentRaster {
                width: 2,
                height: 2,
                rgba8: vec![seed; 16].into(),
            }])
            .0
    }

    /// A transfer whose leave reload restored the startup artwork mid-join still publishes the
    /// join's worker-prepared resources, so the frame never rebuilds them.
    #[test]
    fn transfer_publishes_actor_resources_prepared_by_the_join() {
        for seed in [1, 2] {
            let captured = stack_artwork(1);
            let stack = stack_artwork(seed);
            let base = super::super::resource_packs::entity_artwork_base(&captured, Some(&stack));
            let prepared = PreparedActorArtwork::new_with_block_entity_artwork(base, None, None);
            let mut current = ActorArtworkPages::default();
            assert!(!prepared.matches_sources(&current, None, None));
            install_prepared_artwork(&mut current, Some(&prepared), Some(&stack));
            assert!(prepared.matches_sources(&current, None, None));
            assert_eq!(current.identity(), stack.identity());
        }
    }

    /// Artwork the join already prepared over stays the same snapshot.
    #[test]
    fn matching_artwork_is_not_replaced() {
        let current = stack_artwork(3);
        let prepared = PreparedActorArtwork::new_with_block_entity_artwork(&current, None, None);
        let mut live = current.clone();
        install_prepared_artwork(&mut live, Some(&prepared), Some(&current));
        assert!(live.shares_storage_with(&current));
    }

    /// A transfer that captured the retired server's artwork, to a destination without stack
    /// artwork, keeps the startup artwork the leave reload restored instead of reinstalling the
    /// retired server's textures.
    #[test]
    fn packless_transfer_never_reinstalls_the_retired_servers_artwork() {
        let retired = stack_artwork(1);
        let base = super::super::resource_packs::entity_artwork_base(&retired, None);
        let prepared = PreparedActorArtwork::new_with_block_entity_artwork(base, None, None);
        let startup = ActorArtworkPages::default();
        let mut current = startup.clone();
        install_prepared_artwork(&mut current, Some(&prepared), None);
        assert!(current.shares_storage_with(&startup));
        assert_ne!(current.identity(), retired.identity());
    }

    /// Stack artwork with the same identity as the captured snapshot, as on a transfer between
    /// servers sharing one pack stack, still installs so the frame reuses the join's resources.
    #[test]
    fn same_stack_transfer_installs_the_prepared_base() {
        let captured = stack_artwork(4);
        let stack = stack_artwork(4);
        let base = super::super::resource_packs::entity_artwork_base(&captured, Some(&stack));
        let prepared = PreparedActorArtwork::new_with_block_entity_artwork(base, None, None);
        let mut current = ActorArtworkPages::default();
        install_prepared_artwork(&mut current, Some(&prepared), Some(&stack));
        assert!(prepared.matches_sources(&current, None, None));
        assert_eq!(current.identity(), stack.identity());
    }
}
