//! The sole FIFO committed-UI drain, before frame input authority.
use super::{ClientWorld, WorldClock, record_fatal_error};
use crate::block_cracks::consume_committed_block_crack;
use crate::ui_runtime::{SequencedLocalAttributes, SequencedUiEvent, UiRuntime};
use bevy::{
    prelude::{Res, ResMut, Time},
    time::Real,
};
use client_world::CommittedUiEvent;

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
    ui_runtime.synchronize_local_abilities(&mut player_runtime, session, ability_stream);
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
    ui_runtime.note_stream_dimension(stream.current_dimension());
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
        let known_player_names = stream.player_list_usernames();
        ui_runtime.refresh_raw_text_identities(
            |unique_id| stream.actor_display_name(unique_id),
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
                    ui_runtime.apply_local_abilities(
                        &mut player_runtime,
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
            CommittedUiEvent::Ui { sequence, event } => ui_runtime
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
                .map(|_| ()),
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
            } => ui_runtime.apply_local_attributes(
                &mut player_runtime,
                SequencedLocalAttributes {
                    session_id: clock.session_generation(),
                    fifo_sequence: sequence,
                    local_millis,
                    server_tick,
                    attributes,
                },
            ),
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
            ui_runtime.clear_local_abilities(&mut player_runtime);
            record_fatal_error(
                &mut client_world.fatal_error,
                format!("committed UI/gameplay event rejected: {error:?}"),
            );
            return;
        }
    }
}

#[cfg(test)]
mod tests;
