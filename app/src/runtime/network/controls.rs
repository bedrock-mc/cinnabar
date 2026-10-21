use super::{PackReload, session::NetworkControlEvent};
use crate::{
    game_session::quiesce_local_player,
    runtime::{
        shutdown::record_fatal_error,
        world::{ClientWorld, TransferNotice},
    },
};
use bevy::{
    log::{error, info, warn},
    prelude::ResMut,
};
use client_interaction::movement::{LocalPhysicsController, MovementTicker};
use client_presentation::local_player::{
    InteractionOriginSnapshot, LocalAvatarPresentation, LocalPlayerFrameCarrier,
};
use client_runtime::pack_admission::ResourcePackAdmissionState;
use client_session::{NetworkFailureOrigin, SessionTransferTarget, session_failure_display};
use client_shell::ui_resources::UiRuntimeResource;
use client_state::PlayerRuntime;
use client_ui::ui_runtime::UiRuntime;

/// Why a network session ended; each reason latches its own follow-up.
pub(super) enum SessionEnd {
    /// `remote_close`: the receive side terminated, so the server closed it.
    Failed {
        failure: String,
        remote_close: bool,
    },
    /// The client ends the session to follow the server's transfer.
    Transferred(TransferNotice),
    Stopped,
}

/// Applies acknowledgements and telemetry, or returns a terminal event for retirement.
pub(super) fn apply(
    control: NetworkControlEvent,
    client_world: &mut ClientWorld,
    ui_runtime: &mut ResMut<UiRuntimeResource>,
    movement: &mut ResMut<MovementTicker>,
) -> Option<(SessionEnd, u64)> {
    Some(match control {
        NetworkControlEvent::Bootstrap { .. } => {
            unreachable!("bootstrap is applied before control routing")
        }
        NetworkControlEvent::SubChunkRequestSent { receipt, sent_at } => {
            if let Some(stream) = client_world.stream.as_mut() {
                stream.acknowledge_sub_chunk_request_sent(&receipt, sent_at);
            }
            return None;
        }
        NetworkControlEvent::ChatPacketSent { session, sequence } => {
            if !ui_runtime.acknowledge_chat_send(session, sequence) {
                warn!(
                    session,
                    sequence, "ignored unrelated chat send acknowledgement"
                );
            }
            return None;
        }
        NetworkControlEvent::ChatPacketSendFailed {
            session,
            sequence,
            message,
        } => {
            if ui_runtime.fail_chat_send(session, sequence) {
                error!(session, sequence, "chat packet send failed: {message}");
            } else {
                warn!(
                    session,
                    sequence, "ignored unrelated chat send failure: {message}"
                );
            }
            return None;
        }
        NetworkControlEvent::PhysicsPacketSent { identity } => {
            if !movement.acknowledge_physics_send(identity) {
                warn!(
                    session_generation = identity.session_generation.get(),
                    tick = identity.tick.get(),
                    admission_id = identity.admission_id,
                    reanchor_epoch = identity.reanchor_epoch.get(),
                    "ignored stale, duplicate, or out-of-order physics send acknowledgement"
                );
            }
            return None;
        }
        NetworkControlEvent::PhysicsPacketCancelled {
            identity,
            definitely_unsent,
        } => {
            if !movement.resolve_cancelled_physics_send(identity, definitely_unsent) {
                warn!(
                    session_generation = identity.session_generation.get(),
                    tick = identity.tick.get(),
                    admission_id = identity.admission_id,
                    reanchor_epoch = identity.reanchor_epoch.get(),
                    definitely_unsent,
                    "ignored stale, duplicate, or out-of-order physics cancellation"
                );
            }
            return None;
        }
        NetworkControlEvent::BlobCacheTelemetry { enabled, stats } => {
            client_world.client_blob_cache_enabled = enabled;
            client_world.client_blob_cache = stats;
            return None;
        }
        NetworkControlEvent::Failed {
            message,
            decode_error_count,
            server_disconnect,
            origin,
        } => (
            SessionEnd::Failed {
                failure: session_failure_display(&message, server_disconnect.as_ref()),
                remote_close: origin == NetworkFailureOrigin::Receive,
            },
            decode_error_count,
        ),
        NetworkControlEvent::Transferred {
            target: SessionTransferTarget { host, port },
            decode_error_count,
        } => (
            SessionEnd::Transferred(TransferNotice { host, port }),
            decode_error_count,
        ),
        NetworkControlEvent::Stopped { decode_error_count } => {
            (SessionEnd::Stopped, decode_error_count)
        }
    })
}

/// The session-owned resources cleared by every terminal event.
pub(super) struct Retirement<'a> {
    pub player_runtime: &'a mut PlayerRuntime,
    pub client_world: &'a mut ClientWorld,
    pub fatal_error: &'a mut Option<String>,
    pub ui_runtime: &'a mut UiRuntime,
    pub resource_pack_admission: &'a mut ResourcePackAdmissionState,
    pub pack_reload: Option<&'a mut PackReload>,
    pub movement: &'a mut MovementTicker,
    pub local_physics: &'a mut LocalPhysicsController,
    pub frame: &'a mut LocalPlayerFrameCarrier,
    pub interaction: &'a mut InteractionOriginSnapshot,
    pub avatar: &'a mut LocalAvatarPresentation,
}

impl Retirement<'_> {
    /// Quiesces the old session before latching its failure, transfer or stop outcome.
    pub(super) fn apply(self, end: SessionEnd, decode_error_count: u64) {
        let Self {
            player_runtime,
            client_world,
            fatal_error,
            ui_runtime,
            resource_pack_admission,
            mut pack_reload,
            movement,
            local_physics,
            frame,
            interaction,
            avatar,
        } = self;
        // Every terminal event retires the same session-owned state.
        UiRuntime::retire_crafting_observation();
        render_scene::ViewmodelCompletionGate::retire_observation();
        resource_pack_admission.clear_current();
        if let Some(reload) = pack_reload.as_mut() {
            reload.end_session();
        }
        ui_runtime.set_server_lang(None);
        player_runtime.facts.clear_block_breaking_mode();
        player_runtime.facts.clear_local_abilities();
        // Only a receive-side termination is a remote-initiated close; latch it
        // while the ticker still reports the live session.
        if matches!(
            end,
            SessionEnd::Failed {
                remote_close: true,
                ..
            }
        ) {
            movement.note_remote_session_close();
        }
        quiesce_local_player(movement, local_physics, frame, interaction);
        avatar.clear();
        client_world.network_decode_errors = decode_error_count;
        match end {
            SessionEnd::Failed { failure, .. } => {
                bridge::join_profile::JoinProfile::current().finish("failed");
                error!(decode_error_count, "{failure}");
                record_fatal_error(fatal_error, failure);
            }
            SessionEnd::Transferred(notice) => {
                info!(
                    host = notice.host.as_str(),
                    port = notice.port,
                    "server transferred the session"
                );
                client_world.transfer_notice = Some(notice);
            }
            SessionEnd::Stopped => {
                if fatal_error.is_none() {
                    *fatal_error = Some("network session stopped unexpectedly".into());
                }
            }
        }
    }
}
