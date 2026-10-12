//! Observations of outbound inventory transactions.

use bytes::BytesMut;
use valentine::bedrock::codec::BedrockCodec;
use valentine::bedrock::version::v1_26_51::{
    CerealizerNetworkItemStackDescriptorSerializedData as WireItem, EnumsContainerEnumName,
    EnumsInventorySourceType, EnumsItemReleaseInventoryTransactionActionType,
    EnumsItemUseInventoryTransactionActionType,
    EnumsItemUseInventoryTransactionClientCooldownState,
    EnumsItemUseInventoryTransactionPredictedResult, EnumsItemUseInventoryTransactionTriggerType,
    EnumsItemUseOnActorInventoryTransactionActionType, InventoryAction,
    InventoryTransactionPacketTransaction, McpePacketData, Vec3,
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
    let McpePacketData::InventoryTransactionPacket(packet) = &packet.data else {
        return None;
    };
    let (actions, body) = match &packet.transaction {
        InventoryTransactionPacketTransaction::NormalTransactionData(normal) => {
            (&normal.actions.actions, TransactionBody::Normal)
        }
        InventoryTransactionPacketTransaction::InventoryMismatchData(mismatch) => {
            (&mismatch.actions.actions, TransactionBody::Mismatch)
        }
        InventoryTransactionPacketTransaction::ItemUseInventoryTransaction(body) => (
            &body.actions.actions,
            TransactionBody::ItemUse(ItemUseObservation {
                action: match body.action_type {
                    EnumsItemUseInventoryTransactionActionType::Place => ItemUseAction::ClickBlock,
                    EnumsItemUseInventoryTransactionActionType::Use => ItemUseAction::ClickAir,
                    EnumsItemUseInventoryTransactionActionType::Destroy => {
                        ItemUseAction::BreakBlock
                    }
                    EnumsItemUseInventoryTransactionActionType::Useasattack => {
                        ItemUseAction::UseAsAttack
                    }
                    EnumsItemUseInventoryTransactionActionType::Unknown(value) => {
                        ItemUseAction::Unknown(value)
                    }
                },
                trigger: match body.trigger_type {
                    EnumsItemUseInventoryTransactionTriggerType::Playerinput => {
                        Some(ItemUseTrigger::PlayerInput)
                    }
                    EnumsItemUseInventoryTransactionTriggerType::Simulationtick => {
                        Some(ItemUseTrigger::SimulationTick)
                    }
                    _ => None,
                },
                predicted_success: body.client_interact_prediction
                    == EnumsItemUseInventoryTransactionPredictedResult::Success,
                position: [body.position.x, body.position.y, body.position.z],
                face: body.face,
                slot: body.slot,
                item: item(&body.item),
                from_position: position(&body.from_position),
                click_position: position(&body.click_position),
                target_block_id: body.target_block_id,
                cooldown_active: body.client_cooldown_state
                    == EnumsItemUseInventoryTransactionClientCooldownState::On,
            }),
        ),
        InventoryTransactionPacketTransaction::ItemUseOnActorInventoryTransaction(body) => (
            &body.actions.actions,
            TransactionBody::ItemUseOnActor(ActorUseObservation {
                action: match body.action_type {
                    EnumsItemUseOnActorInventoryTransactionActionType::Attack => {
                        Some(ActorUseAction::Attack)
                    }
                    EnumsItemUseOnActorInventoryTransactionActionType::Interact => {
                        Some(ActorUseAction::Interact)
                    }
                    _ => None,
                },
                runtime_id: body.runtime_id.actor_runtime_id,
                slot: body.slot,
                item: item(&body.item),
                from_position: position(&body.from_position),
                hit_position: position(&body.hit_position),
            }),
        ),
        InventoryTransactionPacketTransaction::ItemReleaseInventoryTransaction(body) => (
            &body.actions.actions,
            TransactionBody::Release(ReleaseObservation {
                action: match body.action_type {
                    EnumsItemReleaseInventoryTransactionActionType::Release => {
                        ReleaseAction::Release
                    }
                    EnumsItemReleaseInventoryTransactionActionType::Use => ReleaseAction::Consume,
                    EnumsItemReleaseInventoryTransactionActionType::Unknown(value) => {
                        ReleaseAction::Unknown(value)
                    }
                },
                slot: body.slot,
                item: item(&body.item),
                from_position: position(&body.from_position),
            }),
        ),
    };
    Some(TransactionObservation {
        legacy_request_id: packet.legacy_request_id.id,
        legacy_set_slots: packet.legacy_set_item_slots.as_ref().map(|slots| {
            slots
                .iter()
                .map(|slot| (container_name(slot.container_enum), slot.slots.clone()))
                .collect()
        }),
        actions: actions.iter().map(action).collect(),
        body,
    })
}

fn action(action: &InventoryAction) -> InventoryActionObservation {
    InventoryActionObservation {
        source: match action.source.source_type {
            EnumsInventorySourceType::Containerinventory => InventorySourceKind::Container,
            EnumsInventorySourceType::Globalinventory => InventorySourceKind::Global,
            EnumsInventorySourceType::Worldinteraction => InventorySourceKind::World,
            EnumsInventorySourceType::Creativeinventory => InventorySourceKind::Creative,
            EnumsInventorySourceType::Nonimplementedfeaturetodo => {
                InventorySourceKind::NotImplemented
            }
            EnumsInventorySourceType::Unknown(value) => InventorySourceKind::Unknown(value),
        },
        container_id: action.source.container_id,
        slot: action.slot,
        from_item: item(&action.from_item),
        to_item: item(&action.to_item),
    }
}

fn item(item: &WireItem) -> ItemObservation {
    ItemObservation {
        network_id: i32::from(item.id),
        count: item.stacksize,
        metadata: item.auxvalue,
        stack_network_id: item.net_id_variant,
        block_runtime_id: item.block_runtime_id,
        user_data: item.user_data_buffer.clone(),
    }
}

fn position(position: &Vec3) -> [f32; 3] {
    [position.x, position.y, position.z]
}

/// The one-byte wire value, matching the crate's `CONTAINER_NAME_*` constants.
fn container_name(name: EnumsContainerEnumName) -> u8 {
    let mut bytes = BytesMut::with_capacity(1);
    name.encode(&mut bytes)
        .expect("a container name encodes into a buffer");
    bytes[0]
}
