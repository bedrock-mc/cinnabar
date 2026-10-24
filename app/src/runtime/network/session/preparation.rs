//! Shared pack and registry preparation at the login bootstrap boundary.
use super::PackApplication;
use std::sync::Arc;

/// Immutable inputs captured by the host before opening one session.
#[derive(Clone)]
pub(crate) struct Preparation {
    pub(crate) registry_base: super::super::session_registries::RegistryBase,
    pub(crate) session_generation: simulation_clock::identity::SessionGeneration,
    pub(crate) actor_artwork: Option<render_model::ActorArtworkPages>,
    pub(crate) ui_catalog: Option<Arc<json_ui::Catalog>>,
    pub(crate) physical_memory_bytes: u64,
}

impl Preparation {
    /// Compiles one join against its captured carriers, abandoning superseded work.
    pub(crate) fn run(
        self,
        handoff: protocol::ResourcePackHandoff,
        game_data: &protocol::GameData,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Option<Result<client_session::PreparedSession<PackApplication>, String>> {
        let profile = bridge::join_profile::JoinProfile::current();
        let _preparation = profile.stage("pack_and_registry_preparation");
        let started = web_time::Instant::now();
        let validation = profile.stage("pack_validation");
        let preparation =
            pack_runtime::prepare_session_packs(handoff, game_data, self.physical_memory_bytes);
        drop(validation);
        if cancelled() {
            return None;
        }
        let compilation = profile.stage("pack_compilation");
        let prepared = super::super::resource_packs::prepare_join(
            &preparation,
            game_data,
            cancelled,
            super::super::resource_packs::JoinBases {
                actor_artwork: self.actor_artwork.as_ref(),
                ui_catalog: self.ui_catalog.as_ref(),
            },
        );
        drop(compilation);
        if preparation.has_applied_packs() && prepared.is_some() {
            bevy::log::info!(
                elapsed_ms = started.elapsed().as_secs_f64() * 1e3,
                "server resource packs prepared for the join"
            );
        }
        let mut packs = match prepared? {
            Ok(packs) => packs,
            Err(error) => return Some(Err(error.to_string())),
        };
        let _registries = profile.stage("session_registries");
        packs.registries = Some(self.registry_base.prepare(
            self.session_generation,
            game_data.world.block_network_ids_are_hashes,
            &preparation.inputs.blocks,
            game_data.items.registry.as_ref().ok(),
            &packs,
            cancelled,
        )?);
        Some(Ok(client_session::PreparedSession {
            custom_blocks: preparation.inputs.blocks.clone(),
            packs_applied: preparation.has_applied_packs(),
            packs,
        }))
    }
}
