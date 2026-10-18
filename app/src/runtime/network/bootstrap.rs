use super::{NetworkLocalPlayerState, PackReload, bootstrap_timing, session::NetworkControlEvent};
use crate::runtime::{shutdown::record_fatal_error, world::AppWorldState};
#[cfg(feature = "acceptance")]
use acceptance::{
    AcceptanceRun, committed_control::acceptance_surface_anchor,
    movement_evidence::MovementEvidenceEventKind,
};
use bevy::{
    log::{debug, info},
    prelude::Mut,
};
use client_presentation::environment::{bind_session_generation, replace_session};
use client_presentation::local_player::{LocalPlayerFrameReset, reset_local_player_session};
use client_runtime::{
    inventory::publish_bootstrap_inventory, pack_admission::ResourcePackAdmissionState,
    publication::PublicationController,
};
use client_state::PlayerRuntime;
use client_world::SAFE_SERVER_HEIGHT;
use pack_runtime::{BootstrapGenerationDisposition, classify_bootstrap_generation};
use render_scene::{BlockEntitySceneResource, ChunkTextureAssets, ChunkUploadAcknowledgements};
use simulation_clock::identity::SessionGeneration;
use std::time::Duration;

/// Pack and publication resources used while installing a StartGame session.
pub(super) struct BootstrapResources<'a> {
    pub player_runtime: Mut<'a, PlayerRuntime>,
    pub resource_pack_admission: Mut<'a, ResourcePackAdmissionState>,
    pub pack_reload: Option<Mut<'a, PackReload>>,
    pub chunk_textures: Option<Mut<'a, ChunkTextureAssets>>,
    pub block_entity_scene: Option<Mut<'a, BlockEntitySceneResource>>,
    pub acknowledgements: &'a ChunkUploadAcknowledgements,
    pub publication: &'a PublicationController,
    pub display_interval: Duration,
    #[cfg(feature = "acceptance")]
    pub acceptance: Mut<'a, AcceptanceRun>,
}

/// Installs session authority, world assets, player state and presentation in dependency order.
pub(super) fn apply(
    control: NetworkControlEvent,
    state: &mut AppWorldState<'_>,
    local_player: &mut NetworkLocalPlayerState<'_>,
    resources: BootstrapResources<'_>,
) {
    let BootstrapResources {
        mut player_runtime,
        mut resource_pack_admission,
        pack_reload,
        chunk_textures,
        block_entity_scene,
        acknowledgements,
        publication,
        display_interval,
        #[cfg(feature = "acceptance")]
        mut acceptance,
    } = resources;
    let AppWorldState {
        client_world,
        fatal_error,
        clock,
        weather,
        movement,
        local_physics,
        movement_effects,
        movement_speed,
        collisions,
        ui_runtime,
        time,
    } = state;
    let NetworkLocalPlayerState {
        view,
        avatar,
        settings,
        frame,
        interaction,
        #[cfg(feature = "acceptance")]
        evidence,
        authority: physics_authority,
        auto_fly,
    } = local_player;
    let NetworkControlEvent::Bootstrap {
        session_generation,
        world: bootstrap,
        environment,
        custom_blocks,
        inventory,
        item_registry,
        player_game_mode,
        world_default_game_mode,
        player_game_mode_uses_world_default,
        server_authoritative_block_breaking,
        rewind_history_size,
        hardcore,
        hud_rules,
        death_rules,
        crafting_rules,
        mut packs,
        terrain_before_spawn,
        level_identity,
    } = control
    else {
        unreachable!("bootstrap routing accepts only StartGame");
    };
    let session_generation = session_generation.get();
    match classify_bootstrap_generation(
        ui_runtime.session_id(),
        clock.session_generation(),
        session_generation,
    ) {
        BootstrapGenerationDisposition::Expected => {}
        BootstrapGenerationDisposition::Stale => {
            client_runtime::session::release_off_frame(packs);
            return;
        }
        BootstrapGenerationDisposition::Unexpected => {
            record_fatal_error(
                &mut fatal_error.0,
                format!(
                    "unexpected StartGame session generation: UI {}, world {}, incoming {session_generation}",
                    ui_runtime.session_id(),
                    clock.session_generation()
                ),
            );
            return;
        }
    }
    let Some(registries) = packs.registries.take() else {
        record_fatal_error(
            &mut fatal_error.0,
            "StartGame block registries were not prepared".to_owned(),
        );
        return;
    };
    if !registries.matches(
        SessionGeneration::new(session_generation),
        bootstrap.block_network_ids_are_hashes,
        &client_world.runtime_assets,
        collisions,
        &custom_blocks,
    ) {
        client_runtime::session::release_off_frame(registries);
        record_fatal_error(
            &mut fatal_error.0,
            "StartGame block registries do not match the active session inputs".to_owned(),
        );
        return;
    }
    ui_runtime.set_server_lang(None);
    player_runtime.facts.clear_block_breaking_mode();
    player_runtime.facts.clear_local_abilities();
    acknowledgements.clear();
    frame.reset(LocalPlayerFrameReset::Session);
    interaction.invalidate();
    #[cfg(feature = "acceptance")]
    evidence.note_event(MovementEvidenceEventKind::Session);
    let mut timings = bootstrap_timing::BootstrapTimings::start();
    info!(
        runtime_id = bootstrap.local_player_runtime_id,
        position = ?bootstrap.player_position,
        world_spawn = ?bootstrap.world_spawn_position,
        "received StartGame bootstrap"
    );
    let replacing_session = clock.session_generation() != 0;
    replace_session(clock, weather, environment, time.elapsed_secs_f64());
    bind_session_generation(clock, weather, session_generation);
    client_runtime::session::begin_session(ui_runtime, &mut player_runtime, session_generation);
    movement_effects.begin_session(session_generation);
    movement_speed.begin_session(session_generation, bootstrap.dimension);
    client_runtime::item_diagnostics::session_registry(item_registry.as_ref());
    let world_item_registry = item_registry.clone();
    if !publish_bootstrap_inventory(&mut player_runtime, ui_runtime, item_registry, inventory) {
        client_runtime::session::release_off_frame(registries);
        record_fatal_error(
            &mut fatal_error.0,
            "StartGame inventory fanout was not an authority event".to_owned(),
        );
        return;
    }
    if let Some(mut reload) = pack_reload {
        reload.begin_session(session_generation, &packs);
    }
    resource_pack_admission.replace_for_generation(
        session_generation,
        std::mem::replace(&mut packs.admission, resource_pack::PackAdmission::None),
    );
    #[cfg(not(target_arch = "wasm32"))]
    {
        ui_runtime.experiences.marker = packs.extension_marker.take();
    }
    player_runtime.facts.publish_bootstrap_game_modes(
        player_game_mode,
        world_default_game_mode
            .hud_mode()
            .unwrap_or(protocol::PlayerGameMode::Unknown),
        player_game_mode_uses_world_default,
    );
    ui_runtime.set_hardcore(hardcore);
    ui_runtime.apply_hud_rules(hud_rules);
    ui_runtime.apply_death_rules(death_rules);
    player_runtime
        .inventory
        .publish_crafting_rules_bootstrap(crafting_rules);
    if replacing_session {
        debug!("replaced StartGame environment session");
    }
    let current = if view.eye_translation().is_finite() {
        view.eye_translation().to_array()
    } else {
        [
            bootstrap.world_spawn_position[0] as f32 + 0.5,
            SAFE_SERVER_HEIGHT,
            bootstrap.world_spawn_position[2] as f32 + 0.5,
        ]
    };
    timings.mark(bootstrap_timing::BootstrapPhase::Session);
    let stream = super::bootstrap_world::build(
        super::bootstrap_world::StreamBootstrap {
            world: bootstrap,
            custom_blocks: &custom_blocks,
            item_registry: world_item_registry,
            world_default_game_mode,
            terrain_before_spawn,
        },
        &packs,
        registries,
        current,
        super::bootstrap_world::StreamResources {
            client_world,
            collisions,
            chunk_textures,
            block_entity_scene,
            publication,
            display_interval,
        },
        &mut timings,
    );
    let resolved = stream.resolved_server_position();
    #[cfg(feature = "acceptance")]
    if acceptance.enabled() {
        acceptance.set_mutation_surface_anchor(acceptance_surface_anchor(resolved.position));
    }
    reset_local_player_session(
        SessionGeneration::new(session_generation),
        bootstrap.local_player_runtime_id,
        resolved.position,
        settings,
        view,
        avatar,
    );
    if let Err(fault) = (gameplay::movement::bootstrap::StartGamePhysics {
        session_generation: clock.session_generation(),
        position: resolved.position,
        rewind_history_size,
        auto_fly: auto_fly.enabled(),
        collision_registry_complete: collisions.is_complete(),
    })
    .apply(***physics_authority, movement, local_physics)
    {
        record_fatal_error(
            &mut fatal_error.0,
            format!("local Physics authority failed closed: {fault:?}"),
        );
    }
    client_world.pending_surface_spawn = resolved.surface_anchor;
    client_world.level_identity = level_identity;
    client_world.stream = Some(stream);
    timings.mark(bootstrap_timing::BootstrapPhase::WorldStream);
    if !super::equipment::publish_bootstrap_equipment(
        &mut player_runtime,
        ui_runtime,
        client_world,
        &mut fatal_error.0,
        session_generation,
        bootstrap.local_player_runtime_id,
    ) {
        return;
    }
    timings.mark(bootstrap_timing::BootstrapPhase::Equipment);
    super::bootstrap_presentation::install(
        &mut packs,
        ui_runtime,
        session_generation,
        fatal_error.0.is_none(),
    );
    player_runtime.facts.install_block_breaking_mode(
        session_generation,
        server_authoritative_block_breaking,
        fatal_error.0.is_none(),
    );
    if let Some(stream) = client_world.stream.as_ref() {
        player_runtime.facts.bind_local_abilities(
            session_generation,
            stream.biome_tint_identity().stream(),
            bootstrap.local_player_unique_id,
            fatal_error.0.is_none(),
        );
    }
    timings.mark(bootstrap_timing::BootstrapPhase::Presentation);
    timings.log();
}
