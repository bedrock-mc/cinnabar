//! The sole FIFO committed-UI drain, before frame input authority.
use super::{ClientWorld, WorldClock, record_fatal_error};
use bevy::{
    prelude::{Res, ResMut, Time},
    time::Real,
};
use chunk_pipeline::WorldStream;
use client_ui::block_cracks::consume_committed_block_crack;
use client_ui::ui_runtime::{SequencedLocalAttributes, SequencedUiEvent, UiRuntime};
use client_world::{CommittedControlEvent, CommittedUiEvent};

pub(crate) fn drain_committed_ui_before_authority(
    mut player_runtime: bevy::prelude::ResMut<crate::player_runtime::PlayerRuntime>,
    mut client_world: ResMut<ClientWorld>,
    clock: Res<WorldClock>,
    mut ui_runtime: ResMut<UiRuntime>,
    time: Res<Time<Real>>,
) {
    let session = clock.session_generation();
    let ability_stream = client_world
        .stream
        .as_ref()
        .filter(|stream| {
            client_world.fatal_error.is_none()
                && client_world.transfer_notice.is_none()
                && stream.inventory_committed_through().is_some()
        })
        .map(|stream| stream.biome_tint_identity().stream());
    player_runtime
        .facts
        .synchronize_local_abilities(session, ability_stream);
    let craft_identity = client_world
        .stream
        .as_ref()
        .filter(|_| client_world.fatal_error.is_none())
        .map(|stream| {
            (
                stream.biome_tint_identity().stream(),
                stream.form_dimension_epoch(),
                stream.inventory_committed_through(),
            )
        });
    ui_runtime.synchronize_crafting_frontier(&mut player_runtime, session, craft_identity);
    let Some(stream) = client_world.stream.as_mut() else {
        return;
    };
    let authority = stream.authority();
    if let Some(name) = authority.actor_display_name(authority.local_player_unique_id()) {
        ui_runtime.set_chat_source_name(name);
    }
    ui_runtime.note_stream_dimension(stream.current_dimension());
    let current_dimension = stream.current_dimension();
    let dimension_epoch = stream.form_dimension_epoch();
    ui_runtime.experiences.epoch = dimension_epoch;
    ui_runtime
        .server_forms_mut()
        .synchronize_epoch(session, dimension_epoch);
    let committed_ui = stream.take_committed_ui();
    let local_millis = u64::try_from(time.elapsed().as_millis()).unwrap_or(u64::MAX);
    if !committed_ui.is_empty() {
        // Rawtext score owners and selectors resolve against the stream's
        // authoritative actor names and player list as of this drain.
        let known_player_names = stream.authority().player_list_usernames();
        ui_runtime.refresh_raw_text_identities(
            |unique_id| stream.authority().actor_display_name(unique_id),
            known_player_names,
        );
    }
    for committed in committed_ui {
        let result = match committed {
            CommittedUiEvent::Experience {
                dimension_epoch: event_epoch,
                event,
                ..
            } => {
                if event_epoch == dimension_epoch {
                    ui_runtime.experiences.receive(&event.bytes, local_millis);
                }
                Ok(())
            }
            CommittedUiEvent::LocalAbilities {
                sequence,
                stream_identity,
                event,
            } => {
                if Some(stream_identity) == ability_stream {
                    player_runtime.facts.apply_local_abilities(
                        session,
                        stream_identity,
                        sequence,
                        event,
                    );
                }
                Ok(())
            }
            CommittedUiEvent::Form {
                sequence,
                dimension_epoch: event_epoch,
                event,
            } => {
                if event_epoch != dimension_epoch {
                    continue;
                }
                ui_runtime
                    .apply(
                        &mut player_runtime,
                        SequencedUiEvent {
                            session_id: session,
                            fifo_sequence: sequence,
                            local_millis,
                            server_tick: None,
                            event: protocol::UiEvent::Form(event),
                        },
                    )
                    .map(|_| ())
            }
            // A generic UI entry must never bypass the form lifetime fence.
            CommittedUiEvent::Ui {
                event: protocol::UiEvent::Form(_),
                ..
            } => continue,
            CommittedUiEvent::Ui { sequence, event } => {
                if let protocol::UiEvent::Boss(boss) = &event {
                    let authority = client_world.stream.as_ref().unwrap().authority();
                    let actor_present = authority
                        .actor_by_unique_id(boss.target_entity_id)
                        .is_some();
                    bevy::log::debug!(
                        sequence,
                        target_entity_id = boss.target_entity_id,
                        action = ?boss.action,
                        actor_present,
                        actors = ?authority.remote_actors().map(|actor| {
                            (actor.unique_id, actor.runtime_id, &actor.kind)
                        }).collect::<Vec<_>>(),
                        "boss actor admission"
                    );
                    // Vanilla resolves the target before dispatch or registration.
                    // A Show before AddActor must not acknowledge a subscription
                    // that cannot yet survive the player's actor-lifetime check.
                    if !actor_present {
                        continue;
                    }
                }
                if let protocol::UiEvent::ShowCredits(credits) = &event {
                    bevy::log::info!(
                        session,
                        sequence,
                        runtime_id = credits.runtime_id,
                        dimension = current_dimension,
                        "committed credits start received"
                    );
                }
                if matches!(
                    event,
                    protocol::UiEvent::Hud(protocol::HudEvent::PlayerStatus(
                        protocol::PlayerStatus::PlayerSpawn
                    ))
                ) {
                    bevy::log::debug!(
                        target: "bedrock_client::runtime::world::dimension",
                        sequence,
                        transfer_active = client_world.dimension_transfer.active(),
                        "player spawn readiness received"
                    );
                }
                ui_runtime
                    .apply(
                        &mut player_runtime,
                        SequencedUiEvent {
                            session_id: clock.session_generation(),
                            fifo_sequence: sequence,
                            local_millis,
                            server_tick: None,
                            event,
                        },
                    )
                    .map(|_| ())
            }
            CommittedUiEvent::BlockCrack {
                sequence,
                dimension,
                event,
            } => consume_committed_block_crack(
                &mut ui_runtime,
                clock.session_generation(),
                sequence,
                dimension,
                event,
            ),
            CommittedUiEvent::LocalAttributes {
                sequence,
                server_tick,
                attributes,
            } => {
                crate::movement::trace_local_attributes(
                    clock.session_generation(),
                    sequence,
                    server_tick,
                    &attributes,
                );
                ui_runtime.apply_local_attributes(
                    &mut player_runtime,
                    SequencedLocalAttributes {
                        session_id: clock.session_generation(),
                        fifo_sequence: sequence,
                        local_millis,
                        server_tick,
                        attributes,
                    },
                )
            }
            CommittedUiEvent::LocalMetadata {
                sequence, metadata, ..
            } => ui_runtime.apply_local_metadata(
                clock.session_generation(),
                sequence,
                metadata.as_ref(),
            ),
            CommittedUiEvent::LocalEffect { sequence, event } => ui_runtime.apply_local_effect(
                clock.session_generation(),
                sequence,
                event,
                local_millis,
            ),
            CommittedUiEvent::LocalMount {
                sequence,
                ridden_unique_id,
            } => ui_runtime.apply_local_mount(
                &mut player_runtime,
                clock.session_generation(),
                sequence,
                ridden_unique_id,
            ),
        };
        if let Err(error) = result {
            player_runtime.facts.clear_local_abilities();
            record_fatal_error(
                &mut client_world.fatal_error,
                format!("committed UI/gameplay event rejected: {error:?}"),
            );
            return;
        }
    }
    if let Some(stream) = client_world.stream.as_ref() {
        ui_runtime.retain_boss_actors(|id| stream.authority().actor_by_unique_id(id).is_some());
    }
}

/// Refreshes Tab/rawtext identity state when a committed player-list marker
/// reports that the authoritative roster changed without a UI packet.
pub(super) fn refresh_player_list_cache_for_controls(
    stream: &WorldStream,
    ui_runtime: &mut UiRuntime,
    controls: &[CommittedControlEvent],
) {
    if !controls
        .iter()
        .any(|control| matches!(control, CommittedControlEvent::PlayerListChanged { .. }))
    {
        return;
    }
    ui_runtime.refresh_raw_text_identities(
        |unique_id| stream.authority().actor_display_name(unique_id),
        stream.authority().player_list_usernames(),
    );
}

#[cfg(test)]
mod tests;
