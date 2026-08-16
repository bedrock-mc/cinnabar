use valentine::bedrock::version::v1_26_40::{
    ContainerClosePacket, EnumsContainerEnumName as FullContainerNameContainerName,
    EnumsItemStackRequestActionType as ItemStackRequestCerealActionType,
    EnumsTextProcessingEventOrigin, FullContainerName, ItemStackRequestCerealPlaceActionData,
    ItemStackRequestCerealSlotInfoData, ItemStackRequestCerealSwapActionData,
    ItemStackRequestCerealTakeActionData, ItemStackRequestPacket,
    ItemStackRequestPacketDataRequestData, ItemStackRequestPacketDataRequestDataActionsItem,
    TypedClientNetIdstructItemStackRequestIdTagint32T0,
};

use super::InventoryPacketError;

pub const PLAYER_INVENTORY_SLOTS: u8 = 36;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum StackRequestContainer {
    PlayerInventory,
    Cursor,
    LevelEntity { dynamic_id: Option<u32> },
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct StackRequestSlot {
    pub container: StackRequestContainer,
    pub slot: u8,
    pub stack_network_id: i32,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum StackRequestAction {
    Take {
        amount: u8,
        source: StackRequestSlot,
        destination: StackRequestSlot,
    },
    Place {
        amount: u8,
        source: StackRequestSlot,
        destination: StackRequestSlot,
    },
    Swap {
        source: StackRequestSlot,
        destination: StackRequestSlot,
    },
}

pub fn item_stack_request_packet(
    request_id: i32,
    action: StackRequestAction,
) -> Result<crate::Packet, InventoryPacketError> {
    if request_id >= -1 || request_id & 1 == 0 {
        return Err(InventoryPacketError::InvalidStackRequestId);
    }
    let action = match action {
        StackRequestAction::Take {
            amount,
            source,
            destination,
        } => {
            validate_request_amount(amount)?;
            ItemStackRequestPacketDataRequestDataActionsItem::TakeActionData(Box::new(
                ItemStackRequestCerealTakeActionData {
                    actiontype: ItemStackRequestCerealActionType::Take,
                    amount,
                    source: request_slot(source)?,
                    destination: request_slot(destination)?,
                },
            ))
        }
        StackRequestAction::Place {
            amount,
            source,
            destination,
        } => {
            validate_request_amount(amount)?;
            ItemStackRequestPacketDataRequestDataActionsItem::PlaceActionData(Box::new(
                ItemStackRequestCerealPlaceActionData {
                    actiontype: ItemStackRequestCerealActionType::Place,
                    amount,
                    source: request_slot(source)?,
                    destination: request_slot(destination)?,
                },
            ))
        }
        StackRequestAction::Swap {
            source,
            destination,
        } => ItemStackRequestPacketDataRequestDataActionsItem::SwapActionData(
            ItemStackRequestCerealSwapActionData {
                actiontype: ItemStackRequestCerealActionType::Swap,
                source: request_slot(source)?,
                destination: request_slot(destination)?,
            },
        ),
    };
    Ok(ItemStackRequestPacket {
        requests: vec![ItemStackRequestPacketDataRequestData {
            client_request_id: TypedClientNetIdstructItemStackRequestIdTagint32T0 {
                id: request_id,
            },
            actions: vec![action],
            strings_to_filter: Vec::new(),
            strings_to_filter_origin: EnumsTextProcessingEventOrigin::Unknown,
        }],
    }
    .into())
}

fn validate_request_amount(amount: u8) -> Result<(), InventoryPacketError> {
    if amount == 0 {
        return Err(InventoryPacketError::InvalidStackRequestAmount);
    }
    Ok(())
}

fn request_slot(
    slot: StackRequestSlot,
) -> Result<ItemStackRequestCerealSlotInfoData, InventoryPacketError> {
    let container_name = match slot.container {
        StackRequestContainer::PlayerInventory if slot.slot < PLAYER_INVENTORY_SLOTS => {
            FullContainerNameContainerName::CombinedHotbarAndInventoryContainer
        }
        StackRequestContainer::Cursor if slot.slot == 0 => {
            FullContainerNameContainerName::CursorContainer
        }
        StackRequestContainer::LevelEntity { .. } => {
            FullContainerNameContainerName::LevelEntityContainer
        }
        _ => {
            return Err(InventoryPacketError::InvalidStackRequestSlot {
                container: slot.container,
                slot: slot.slot,
            });
        }
    };
    if slot.stack_network_id < -1 {
        return Err(InventoryPacketError::InvalidRequestStackNetworkId(
            slot.stack_network_id,
        ));
    }
    Ok(ItemStackRequestCerealSlotInfoData {
        fullcontainername: FullContainerName {
            container_name,
            dynamic_id: match slot.container {
                StackRequestContainer::LevelEntity { dynamic_id } => dynamic_id,
                _ => None,
            },
        },
        slot: slot.slot,
        net_id_variant: slot.stack_network_id,
    })
}

pub fn container_close_packet(
    window_id: i32,
    window_type: i8,
) -> Result<crate::Packet, InventoryPacketError> {
    let container_id = match window_id {
        -128..=-1 => (window_id as i8).to_ne_bytes()[0],
        0..=255 => window_id as u8,
        _ => {
            return Err(InventoryPacketError::InvalidContainerCloseWindowId(
                window_id,
            ));
        }
    };
    Ok(ContainerClosePacket {
        container_id,
        container_type: window_type.to_ne_bytes()[0],
        server_initiated_close: false,
    }
    .into())
}
