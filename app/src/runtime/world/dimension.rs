//! Dimension acknowledgement after the committed flush, separate from terrain presentation.

use std::time::Duration;

use protocol::{ChangeDimensionEvent, LoadingScreenPhase, Packet};

use crate::runtime::network::{NetworkHandle, PacketSendError};

/// Transfer control packets keep progressing when the window cannot present.
pub(crate) fn advance_dimension_transfer(
    mut client_world: bevy::prelude::ResMut<super::ClientWorld>,
    network: bevy::prelude::Res<NetworkHandle>,
    clock: bevy::prelude::Res<crate::environment::WorldClock>,
    time: bevy::prelude::Res<bevy::prelude::Time<bevy::time::Real>>,
    mut menu: Option<bevy::prelude::ResMut<crate::menu::MenuRuntime>>,
) {
    let session = client_world
        .stream
        .as_ref()
        .map(|_| clock.session_generation());
    client_world.dimension_transfer.synchronize_session(session);
    client_world.respawn.synchronize_session(session);
    let transferring = client_world.dimension_transfer.active();
    if let Some(stream) = client_world.stream.as_mut() {
        let priority = transferring.then(|| stream.resolved_server_position().position);
        stream.set_dimension_transfer_priority(priority);
    }
    if transferring && let Some(menu) = menu.as_deref_mut() {
        menu.set_visible(false);
    }
    if client_world.dimension_transfer.log_wait_due(time.elapsed())
        && let Some(stream) = &client_world.stream
    {
        bevy::log::debug!(
            position = ?stream.resolved_server_position().position,
            destination_decoded = stream.dimension_transfer_ready(stream.resolved_server_position().position),
            destination_presentable = stream.dimension_transfer_presentable(stream.resolved_server_position().position),
            local_terrain_ready = stream.local_terrain_ready(),
            loaded_columns = stream.loaded_column_count(),
            full_client_offsets_loaded = stream.dimension_loading_columns_ready(),
            missing_loading_columns = ?stream.dimension_loading_missing_columns(),
            "dimension loading wait"
        );
    }
    if let Err(error) = client_world.dimension_transfer.queue_switch(&network) {
        if error.is_closed() {
            super::record_fatal_error(
                &mut client_world.fatal_error,
                format!("dimension transfer: {error}"),
            );
        }
        return;
    }
    if let Err(error) = client_world
        .respawn
        .queue_completion(|packet| network.send_dimension_packet(packet))
        && error.is_closed()
    {
        super::record_fatal_error(
            &mut client_world.fatal_error,
            format!("respawn completion: {error}"),
        );
    }
}

#[derive(Debug, Default)]
pub(crate) struct DimensionTransfer {
    active: Option<PendingTransfer>,
}

#[derive(Debug)]
struct PendingTransfer {
    session: u64,
    epoch: u64,
    change: ChangeDimensionEvent,
    actor: u64,
    presentation_pending: bool,
    start_queued: bool,
    server_acknowledged: bool,
    switch_queued: bool,
    last_wait_log: Duration,
}

impl DimensionTransfer {
    pub(crate) fn begin(
        &mut self,
        session: u64,
        epoch: u64,
        change: ChangeDimensionEvent,
        actor: u64,
        now: Duration,
    ) {
        bevy::log::debug!(session, epoch, dimension = change.dimension, loading_screen_id = ?change.loading_screen_id, "dimension transfer started");
        self.active = Some(PendingTransfer {
            session,
            epoch,
            change,
            actor,
            presentation_pending: true,
            start_queued: false,
            server_acknowledged: false,
            switch_queued: false,
            last_wait_log: now,
        });
    }

    pub(crate) fn acknowledge(&mut self, epoch: u64) {
        if let Some(pending) = &mut self.active
            && pending.epoch == epoch
            && !pending.server_acknowledged
        {
            pending.server_acknowledged = true;
            bevy::log::debug!(epoch, "dimension server acknowledgement received");
        }
    }

    pub(crate) fn synchronize_session(&mut self, session: Option<u64>) {
        if self
            .active
            .as_ref()
            .is_some_and(|pending| Some(pending.session) != session)
        {
            self.active = None;
        }
    }

    pub(crate) fn take_presentation_reset(&mut self) -> bool {
        self.active
            .as_mut()
            .is_some_and(|pending| std::mem::take(&mut pending.presentation_pending))
    }

    pub(crate) fn active(&self) -> bool {
        self.active.is_some()
    }

    fn log_wait_due(&mut self, now: Duration) -> bool {
        let Some(pending) = &mut self.active else {
            return false;
        };
        if !bevy::log::tracing::enabled!(bevy::log::Level::DEBUG)
            || now.saturating_sub(pending.last_wait_log) < Duration::from_secs(1)
        {
            return false;
        }
        pending.last_wait_log = now;
        true
    }

    pub(crate) fn waiting_for_switch(&self) -> bool {
        self.active
            .as_ref()
            .is_some_and(|pending| !pending.switch_queued)
    }

    fn next_before_presentation(&self) -> Option<Packet> {
        let pending = self.active.as_ref()?;
        if !pending.start_queued {
            return Some(protocol::loading_screen_packet(
                LoadingScreenPhase::Start,
                pending.change.loading_screen_id,
            ));
        }
        // Action 14 clears the server's movement wait without a terrain prerequisite.
        // Local movement stays held until destination presentation completes.
        (!pending.switch_queued).then(|| protocol::dimension_change_done_packet(pending.actor))
    }

    pub(crate) fn queue_switch(&mut self, network: &NetworkHandle) -> Result<(), PacketSendError> {
        self.send_before_presentation(|packet| network.send_dimension_packet(packet))
    }

    fn send_before_presentation(
        &mut self,
        mut send: impl FnMut(Packet) -> Result<(), PacketSendError>,
    ) -> Result<(), PacketSendError> {
        // At most Start then Switch; queue saturation retries the unsent step.
        while let Some(packet) = self.next_before_presentation() {
            send(packet)?;
            let pending = self
                .active
                .as_mut()
                .expect("active transfer produced packet");
            if pending.start_queued {
                pending.switch_queued = true;
                bevy::log::debug!(
                    epoch = pending.epoch,
                    runtime_id = pending.actor,
                    "dimension done queued after committed flush"
                );
            } else {
                pending.start_queued = true;
                bevy::log::debug!(epoch = pending.epoch, loading_screen_id = ?pending.change.loading_screen_id, "dimension loading screen start queued");
            }
        }
        Ok(())
    }

    pub(crate) fn finish_presentation(
        &mut self,
        network: &NetworkHandle,
    ) -> Result<bool, PacketSendError> {
        self.complete_with(|packet| network.send_dimension_packet(packet))
    }

    fn complete_with(
        &mut self,
        send: impl FnOnce(Packet) -> Result<(), PacketSendError>,
    ) -> Result<bool, PacketSendError> {
        let Some(pending) = &self.active else {
            return Ok(true);
        };
        if !pending.switch_queued {
            return Ok(false);
        }
        send(protocol::loading_screen_packet(
            LoadingScreenPhase::End,
            pending.change.loading_screen_id,
        ))?;
        bevy::log::debug!(epoch = pending.epoch, loading_screen_id = ?pending.change.loading_screen_id, "dimension loading screen end queued after presentation readiness");
        self.active = None;
        Ok(true)
    }
}

#[cfg(test)]
#[path = "dimension_tests.rs"]
mod tests;
