//! Immutable presentation projection of ordered world cracking authority.

use crate::ui_runtime::{SequencedBlockCrackEvent, UiRuntime, UiRuntimeError};
use protocol::BlockCrackEvent;

pub use chunk_pipeline::BlockCrackStatus;

pub fn consume_committed_block_crack(
    ui: &mut UiRuntime,
    session_id: u64,
    sequence: u64,
    dimension: i32,
    event: BlockCrackEvent,
) -> Result<(), UiRuntimeError> {
    // The world stream already consumed this event synchronously at its FIFO
    // commit. Here only identity is validated: replaying admission would lose
    // accepted starts when a preceding world mutation freed a full ledger.
    ui.retain_block_crack(SequencedBlockCrackEvent {
        session_id,
        fifo_sequence: sequence,
        dimension,
        event,
    })
}

pub fn reconcile_world_block_cracks(ui: &mut UiRuntime, stream: &chunk_pipeline::WorldStream) {
    ui.project_block_cracks(stream.block_crack_snapshot());
}

#[derive(Clone, Debug, Default)]
pub struct BlockCracks {
    snapshot: Option<chunk_pipeline::BlockCrackSnapshot>,
    reported_status: Option<BlockCrackStatus>,
}

impl BlockCracks {
    pub fn status(&self) -> BlockCrackStatus {
        self.snapshot
            .as_ref()
            .map_or_else(BlockCrackStatus::default, |snapshot| snapshot.status)
    }

    /// The active cracks and the dimension they belong to; empty before the first projection.
    pub fn snapshot(&self) -> Option<&chunk_pipeline::BlockCrackSnapshot> {
        self.snapshot.as_ref()
    }

    pub fn synchronize_dimension(&mut self, dimension: Option<i32>) {
        if self
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| Some(snapshot.dimension) != dimension)
        {
            self.snapshot = None;
        }
    }

    pub fn project(&mut self, snapshot: chunk_pipeline::BlockCrackSnapshot) {
        self.snapshot = Some(snapshot);
    }

    pub fn report_status(&mut self, session_id: u64, status: BlockCrackStatus) {
        let previous = self.reported_status;
        if previous == Some(status) {
            return;
        }
        self.reported_status = Some(status);
        if status.capacity_rejections > previous.map_or(0, |status| status.capacity_rejections) {
            bevy::log::warn!(target: "bedrock_client::block_cracks",
                active = status.active, rejected = status.capacity_rejections,
                "block crack active capacity exhausted");
        }
        bevy::log::debug!(target: "bedrock_client::block_cracks", session_id,
            active = status.active, server_value_sum = status.server_value_sum,
            consumed = status.consumed, orphan_updates = status.orphan_updates,
            capacity_rejections = status.capacity_rejections,
            unsupported_values = status.unsupported_values,
            unsupported_targets = status.unsupported_targets,
            retired_targets = status.retired_targets, "block crack state consumed");
    }
}

#[cfg(test)]
mod tests;
