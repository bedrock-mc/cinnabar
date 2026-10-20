use crate::runtime::world::AppWorldState;
use bevy::{
    ecs::system::SystemParam,
    prelude::{Res, ResMut},
};
use client_interaction::movement::PhysicsAuthorityGate;
use client_presentation::{
    camera::{AutoFly, CameraSettingsAuthority},
    local_player::{
        InteractionOriginSnapshot, LocalAvatarPresentation, LocalPlayerFrameCarrier, LocalViewPose,
    },
};
use client_runtime::{
    ingress::{NETWORK_CONTROL_BUDGET_PER_FRAME, drain_network_controls},
    network::NetworkHandle,
    pack_admission::ResourcePackAdmissionState,
    publication::PublicationController,
};
use render_scene::{
    ChunkTextureAssets, ChunkUploadAcknowledgements, RuntimeStage, RuntimeStageProfiler,
};
use resource_packs::PackApplication;
use session::NetworkControlEvent;
use std::time::Duration;
#[cfg(feature = "acceptance")]
use {
    crate::runtime::{movement_evidence::MovementEvidenceEmitter, visibility::AppMetrics},
    acceptance::{AcceptanceRun, model_witness::ModelWitnessFileSource},
};

pub(crate) use pack_reload::{PackReload, reload_resource_packs};
pub(crate) use resource_packs::reuse::CompiledStacks;
pub(crate) use resource_packs::ui_catalog::PackUiCatalog;
pub(crate) use resource_packs::{
    await_overlay_texture_device, compiled_stacks, initialize_overlay_texture_limit,
    set_base_material_keys,
};

#[derive(SystemParam)]
pub(crate) struct NetworkLocalPlayerState<'w> {
    view: ResMut<'w, LocalViewPose>,
    avatar: ResMut<'w, LocalAvatarPresentation>,
    settings: ResMut<'w, CameraSettingsAuthority>,
    frame: ResMut<'w, LocalPlayerFrameCarrier>,
    interaction: ResMut<'w, InteractionOriginSnapshot>,
    #[cfg(feature = "acceptance")]
    evidence: ResMut<'w, MovementEvidenceEmitter>,
    authority: Res<'w, PhysicsAuthorityGate>,
    auto_fly: Res<'w, AutoFly>,
}

// Explicit Bevy access modes let the scheduler validate conflicts while bootstrap
// shares the publication allowance with each new world stream.
#[allow(clippy::too_many_arguments)]
pub(crate) fn receive_network_events(
    mut player_runtime: bevy::prelude::ResMut<client_state::PlayerRuntime>,
    mut network: ResMut<NetworkHandle<PackApplication>>,
    mut resource_pack_admission: ResMut<ResourcePackAdmissionState>,
    mut pack_reload: Option<ResMut<PackReload>>,
    mut chunk_textures: Option<ResMut<ChunkTextureAssets>>,
    mut block_entity_scene: Option<ResMut<render_scene::BlockEntitySceneResource>>,
    mut state: AppWorldState,
    #[cfg(feature = "acceptance")] mut acceptance: ResMut<AcceptanceRun>,
    #[cfg(feature = "acceptance")] metrics: Res<AppMetrics>,
    acknowledgements: Res<ChunkUploadAcknowledgements>,
    #[cfg(feature = "acceptance")] model_witness_source: Res<ModelWitnessFileSource>,
    publication: Res<PublicationController>,
    mut local_player: NetworkLocalPlayerState,
    profiler: Option<Res<RuntimeStageProfiler>>,
) {
    let _join_profile = network.enter_join_profile();
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(RuntimeStage::NetworkIngestion));
    let display_interval = profiler
        .as_deref()
        .map_or(Duration::ZERO, RuntimeStageProfiler::frame_interval);
    if let Some(stream) = state.client_world.stream.as_mut() {
        stream.set_display_interval(display_interval);
        stream.begin_frame_work();
    }
    // Controls establish or retire the session before world ingress. Bootstrap publishes
    // the stream before releasing equipment; each world event then sees the preceding commit.
    for control in drain_network_controls(
        network.control_events_mut(),
        NETWORK_CONTROL_BUDGET_PER_FRAME,
    ) {
        if matches!(control, NetworkControlEvent::Bootstrap { .. }) {
            bridge::join_profile::JoinProfile::current().mark("bootstrap_received");
            bootstrap::apply(
                control,
                &mut state,
                &mut local_player,
                bootstrap::BootstrapResources {
                    player_runtime: player_runtime.reborrow(),
                    resource_pack_admission: resource_pack_admission.reborrow(),
                    pack_reload: pack_reload.as_mut().map(|reload| reload.reborrow()),
                    chunk_textures: chunk_textures.as_mut().map(|textures| textures.reborrow()),
                    block_entity_scene: block_entity_scene.as_mut().map(|scene| scene.reborrow()),
                    acknowledgements: &acknowledgements,
                    publication: &publication,
                    display_interval,
                    #[cfg(feature = "acceptance")]
                    acceptance: acceptance.reborrow(),
                },
            );
        } else if let Some((end, decode_error_count)) = controls::apply(
            control,
            &mut state.client_world,
            &mut state.ui_runtime,
            &mut state.movement,
        ) {
            controls::Retirement {
                player_runtime: &mut player_runtime,
                client_world: &mut state.client_world,
                fatal_error: &mut state.fatal_error.0,
                ui_runtime: &mut state.ui_runtime,
                resource_pack_admission: &mut resource_pack_admission,
                pack_reload: pack_reload.as_deref_mut(),
                movement: &mut state.movement,
                local_physics: &mut state.local_physics,
                frame: &mut local_player.frame,
                interaction: &mut local_player.interaction,
                avatar: &mut local_player.avatar,
            }
            .apply(end, decode_error_count);
        }
    }
    world_ingress::drain(
        &mut network,
        &mut state.client_world,
        &mut state.fatal_error.0,
        &mut player_runtime,
        &mut state.ui_runtime,
        #[cfg(feature = "acceptance")]
        &mut acceptance,
        #[cfg(feature = "acceptance")]
        &metrics,
        #[cfg(feature = "acceptance")]
        &model_witness_source,
    );
}

#[cfg(test)]
mod actor_test_support;
mod bootstrap;
mod bootstrap_presentation;
mod bootstrap_world;
mod controls;
pub(crate) mod equipment;
mod world_ingress;
#[cfg(test)]
pub(crate) use actor_test_support::{actor_render_source, update_actor_render_scene};

mod actor_publication;
#[cfg(test)]
pub(crate) use actor_publication::custom_emotes::fixture as actor_publication_test_world;
mod block_overlay;
mod bootstrap_timing;
mod compile_execution;
pub(crate) mod entity_pack;
mod entity_texture_reload;
pub(crate) mod glyph_sheets;
mod item_icons;
mod server_animation;
pub(crate) use item_icons::install_vanilla_item_assets;
#[cfg(test)]
mod join_first_frame_tests;
#[cfg(test)]
mod local_pack;
mod pack_reload;
#[cfg(test)]
mod pack_reload_diff_tests;
#[cfg(test)]
mod pack_reload_tests;
#[cfg(test)]
mod pack_reload_world_witness;
pub(crate) use entity_texture_reload::set_base_actor_artwork;
pub(crate) mod block_entity_art;
pub(crate) mod reload_environment;
pub(crate) mod resource_packs;
pub(crate) mod session;
pub(crate) mod session_registries;
pub(crate) use actor_publication::{
    advance_actor_frame, advance_actor_motion, initialize_actor_geometry_budget,
    prepare_actor_render_frame, publish_entity_shadows, publish_local_actor_damage,
};

#[cfg(test)]
#[path = "network/drain/tests.rs"]
mod drain_tests;
