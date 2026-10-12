use bedrock_protocol_1_26_51::enums::{ItemStackRequestActionType, TextProcessingEventOrigin};
use bedrock_protocol_1_26_51::types::{
    ItemStackRequestData, ItemStackRequestID, StackRequestAction,
};
use bedrock_protocol_1_26_51::wire;
use thiserror::Error;

/// A bounded mining request carrier, not proof of current inventory authority.
/// Callers must independently establish item applicability and current state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MineBlockRequest {
    request_id: i32,
    hotbar_slot: u8,
    predicted_durability: i32,
    stack_network_id: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum MineBlockRequestError {
    #[error("mining request ID must be odd and at most -3")]
    InvalidRequestId,
    #[error("mining hotbar slot must be within 0..=8")]
    InvalidHotbarSlot,
    #[error("predicted mining durability must be nonnegative")]
    InvalidPredictedDurability,
    #[error("mining stack network ID must be positive")]
    InvalidStackNetworkId,
}

impl MineBlockRequest {
    #[must_use]
    pub const fn request_id(&self) -> i32 {
        self.request_id
    }

    pub fn new(
        request_id: i32,
        hotbar_slot: u8,
        predicted_durability: i32,
        stack_network_id: i32,
    ) -> Result<Self, MineBlockRequestError> {
        if request_id > -3 || request_id & 1 == 0 {
            return Err(MineBlockRequestError::InvalidRequestId);
        }
        if hotbar_slot > 8 {
            return Err(MineBlockRequestError::InvalidHotbarSlot);
        }
        if predicted_durability < 0 {
            return Err(MineBlockRequestError::InvalidPredictedDurability);
        }
        if stack_network_id <= 0 {
            return Err(MineBlockRequestError::InvalidStackNetworkId);
        }
        Ok(Self {
            request_id,
            hotbar_slot,
            predicted_durability,
            stack_network_id,
        })
    }

    pub(crate) fn packed(self) -> ItemStackRequestData {
        ItemStackRequestData {
            client_request_id: ItemStackRequestID(self.request_id),
            actions: vec![StackRequestAction::MineBlockActionData {
                action_type: ItemStackRequestActionType::Screenhudmineblock,
                slot: wire::ZigZag32(i32::from(self.hotbar_slot)),
                predicted_durability: wire::ZigZag32(self.predicted_durability),
                net_id_variant: wire::I32LE(self.stack_network_id),
            }],
            strings_to_filter: Vec::new(),
            strings_to_filter_origin: TextProcessingEventOrigin::Unknown,
        }
    }
}
