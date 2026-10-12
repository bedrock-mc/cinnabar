//! Observations of outbound inventory transactions.

use bedrock_protocol_1_26_51::enums::{
    InventorySourceType, ItemReleaseInventoryTransactionActionType,
    ItemUseInventoryTransactionActionType, ItemUseInventoryTransactionClientCooldownState,
    ItemUseInventoryTransactionPredictedResult, ItemUseInventoryTransactionTriggerType,
    ItemUseOnActorInventoryTransactionActionType,
};
use bedrock_protocol_1_26_51::packets;
use bedrock_protocol_1_26_51::types::{
    InventoryAction, InventoryTransactionPacketData,
    NetworkItemStackDescriptorSerializedData as WireItem,
};

use crate::{ActorUseAction, ItemUseTrigger, Packet};

/// One outbound InventoryTransaction, read back into owned values.
#[derive(Clone, Debug, PartialEq)]
pub struct TransactionObservation {
    pub legacy_request_id: i32,
    /// `(container name, slots)` the legacy request claims; `None` when the packet omits them.
    pub legacy_set_slots: Option<Vec<(u8, Vec<u8>)>>,
    /// The transaction's inventory actions, in wire order.
    pub actions: Vec<InventoryActionObservation>,
    pub body: TransactionBody,
}

/// Which transaction body the packet carries.
#[derive(Clone, Debug, PartialEq)]
pub enum TransactionBody {
    Normal,
    Mismatch,
    ItemUse(ItemUseObservation),
    ItemUseOnActor(ActorUseObservation),
    Release(ReleaseObservation),
}

/// One inventory action's source, slot and stacks.
#[derive(Clone, Debug, PartialEq)]
pub struct InventoryActionObservation {
    pub source: InventorySourceKind,
    pub container_id: Option<i8>,
    pub slot: u32,
    pub from_item: ItemObservation,
    pub to_item: ItemObservation,
}

/// Where an inventory action's change happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InventorySourceKind {
    Container,
    Global,
    World,
    Creative,
    NotImplemented,
    Unknown(u32),
}

/// A stack descriptor as the wire carries it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemObservation {
    pub network_id: i32,
    pub count: u16,
    pub metadata: u32,
    pub stack_network_id: Option<i32>,
    pub block_runtime_id: u32,
    pub user_data: Vec<u8>,
}

/// An item-use transaction against a block or the air.
#[derive(Clone, Debug, PartialEq)]
pub struct ItemUseObservation {
    pub action: ItemUseAction,
    /// `None` for a trigger outside player input and simulation ticks.
    pub trigger: Option<ItemUseTrigger>,
    pub predicted_success: bool,
    pub position: [i32; 3],
    pub face: u8,
    pub slot: i32,
    pub item: ItemObservation,
    pub from_position: [f32; 3],
    pub click_position: [f32; 3],
    pub target_block_id: u32,
    pub cooldown_active: bool,
}

/// What an item-use transaction asks the server to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemUseAction {
    ClickBlock,
    ClickAir,
    BreakBlock,
    UseAsAttack,
    Unknown(i32),
}

/// An item-use-on-actor transaction.
#[derive(Clone, Debug, PartialEq)]
pub struct ActorUseObservation {
    /// `None` for an action other than attack or interact.
    pub action: Option<ActorUseAction>,
    pub runtime_id: u64,
    pub slot: i32,
    pub item: ItemObservation,
    pub from_position: [f32; 3],
    pub hit_position: [f32; 3],
}

/// A release-item transaction.
#[derive(Clone, Debug, PartialEq)]
pub struct ReleaseObservation {
    pub action: ReleaseAction,
    pub slot: i32,
    pub item: ItemObservation,
    pub from_position: [f32; 3],
}

/// Whether a released item was let go or consumed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReleaseAction {
    Release,
    Consume,
    Unknown(i32),
}

impl TransactionObservation {
    /// The item-use body, if this is one.
    pub fn item_use(&self) -> Option<&ItemUseObservation> {
        match &self.body {
            TransactionBody::ItemUse(body) => Some(body),
            _ => None,
        }
    }

    /// The item-use-on-actor body, if this is one.
    pub fn actor_use(&self) -> Option<&ActorUseObservation> {
        match &self.body {
            TransactionBody::ItemUseOnActor(body) => Some(body),
            _ => None,
        }
    }

    /// The release body, if this is one.
    pub fn release(&self) -> Option<&ReleaseObservation> {
        match &self.body {
            TransactionBody::Release(body) => Some(body),
            _ => None,
        }
    }
}

/// Reads an InventoryTransaction packet; `None` for any other packet.
pub fn observe_transaction(packet: &Packet) -> Option<TransactionObservation> {
    let packets::Packet::InventoryTransaction(packet) = &*packet.data else {
        return None;
    };
    let (actions, body) = match &packet.transaction {
        InventoryTransactionPacketData::NormalTransactionData { actions } => {
            (&actions.actions, TransactionBody::Normal)
        }
        InventoryTransactionPacketData::InventoryMismatchData { actions } => {
            (&actions.actions, TransactionBody::Mismatch)
        }
        InventoryTransactionPacketData::ItemUseInventoryTransaction(body) => (
            &body.actions.actions,
            TransactionBody::ItemUse(ItemUseObservation {
                action: match body.action_type {
                    ItemUseInventoryTransactionActionType::Place => ItemUseAction::ClickBlock,
                    ItemUseInventoryTransactionActionType::Use => ItemUseAction::ClickAir,
                    ItemUseInventoryTransactionActionType::Destroy => ItemUseAction::BreakBlock,
                    ItemUseInventoryTransactionActionType::UseAsAttack => {
                        ItemUseAction::UseAsAttack
                    }
                    ItemUseInventoryTransactionActionType::Unknown(value) => {
                        ItemUseAction::Unknown(value)
                    }
                },
                trigger: match body.trigger_type {
                    ItemUseInventoryTransactionTriggerType::PlayerInput => {
                        Some(ItemUseTrigger::PlayerInput)
                    }
                    ItemUseInventoryTransactionTriggerType::SimulationTick => {
                        Some(ItemUseTrigger::SimulationTick)
                    }
                    _ => None,
                },
                predicted_success: body.client_interact_prediction
                    == ItemUseInventoryTransactionPredictedResult::Success,
                position: [body.position.x.0, body.position.y.0, body.position.z.0],
                face: body.face.0,
                slot: body.slot.0,
                item: item(&body.item),
                from_position: body.from_position.to_array(),
                click_position: body.click_position.to_array(),
                target_block_id: body.target_block_id.0,
                cooldown_active: body.client_cooldown_state
                    == ItemUseInventoryTransactionClientCooldownState::On,
            }),
        ),
        InventoryTransactionPacketData::ItemUseOnActorInventoryTransaction {
            actions,
            runtime_id,
            action_type,
            slot,
            item: used,
            from_position,
            hit_position,
        } => (
            &actions.actions,
            TransactionBody::ItemUseOnActor(ActorUseObservation {
                action: match action_type {
                    ItemUseOnActorInventoryTransactionActionType::Attack => {
                        Some(ActorUseAction::Attack)
                    }
                    ItemUseOnActorInventoryTransactionActionType::Interact => {
                        Some(ActorUseAction::Interact)
                    }
                    _ => None,
                },
                runtime_id: runtime_id.0,
                slot: slot.0,
                item: item(used),
                from_position: from_position.to_array(),
                hit_position: hit_position.to_array(),
            }),
        ),
        InventoryTransactionPacketData::ItemReleaseInventoryTransaction {
            actions,
            action_type,
            slot,
            item: released,
            from_position,
        } => (
            &actions.actions,
            TransactionBody::Release(ReleaseObservation {
                action: match action_type {
                    ItemReleaseInventoryTransactionActionType::Release => ReleaseAction::Release,
                    ItemReleaseInventoryTransactionActionType::Use => ReleaseAction::Consume,
                    ItemReleaseInventoryTransactionActionType::Unknown(value) => {
                        ReleaseAction::Unknown(*value)
                    }
                },
                slot: slot.0,
                item: item(released),
                from_position: from_position.to_array(),
            }),
        ),
    };
    Some(TransactionObservation {
        legacy_request_id: packet.legacy_request_id.0,
        legacy_set_slots: packet.legacy_set_item_slots.as_ref().map(|slots| {
            slots
                .iter()
                .map(|slot| (slot.container_enum.to_raw(), slot.slots.to_vec()))
                .collect()
        }),
        actions: actions.iter().map(action).collect(),
        body,
    })
}

fn action(action: &InventoryAction) -> InventoryActionObservation {
    InventoryActionObservation {
        source: match action.source.source_type {
            InventorySourceType::ContainerInventory => InventorySourceKind::Container,
            InventorySourceType::GlobalInventory => InventorySourceKind::Global,
            InventorySourceType::WorldInteraction => InventorySourceKind::World,
            InventorySourceType::CreativeInventory => InventorySourceKind::Creative,
            InventorySourceType::NonImplementedFeatureTodo => InventorySourceKind::NotImplemented,
            InventorySourceType::Unknown(value) => InventorySourceKind::Unknown(value),
        },
        container_id: action.source.container_id.map(|id| id.0),
        slot: action.slot.0,
        from_item: item(&action.from_item),
        to_item: item(&action.to_item),
    }
}

fn item(item: &WireItem) -> ItemObservation {
    ItemObservation {
        network_id: i32::from(item.id.0),
        count: item.stack_size.0,
        metadata: item.aux_value.0,
        stack_network_id: item.net_id_variant.map(|id| id.0),
        block_runtime_id: item.block_runtime_id.0,
        user_data: item.user_data_buffer.to_vec(),
    }
}
