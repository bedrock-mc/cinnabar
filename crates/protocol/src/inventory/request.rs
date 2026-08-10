use valentine::bedrock::version::v1_26_40::{
    FullContainerName, FullContainerNameContainerName, ItemStackRequestCerealPlaceActionData,
    ItemStackRequestCerealPlaceActionDataActiontype, ItemStackRequestCerealSlotInfoData,
    ItemStackRequestCerealSwapActionData, ItemStackRequestCerealSwapActionDataActiontype,
    ItemStackRequestCerealTakeActionData, ItemStackRequestCerealTakeActionDataActiontype,
    ItemStackRequestPacket, ItemStackRequestPacketDataRequestData,
    ItemStackRequestPacketDataRequestDataActionsItem,
    ItemStackRequestPacketDataRequestDataStringsToFilterOrigin,
    TypedClientNetIdStructItemStackRequestIdTagInt32T0,
};

use super::InventoryPacketError;

pub const PLAYER_INVENTORY_SLOTS: u8 = 36;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum StackRequestContainer {
    PlayerInventory,
    Cursor,
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
    if request_id <= 0 {
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
                    actiontype: ItemStackRequestCerealTakeActionDataActiontype::Take,
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
                    actiontype: ItemStackRequestCerealPlaceActionDataActiontype::Place,
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
                actiontype: ItemStackRequestCerealSwapActionDataActiontype::Swap,
                source: request_slot(source)?,
                destination: request_slot(destination)?,
            },
        ),
    };
    Ok(ItemStackRequestPacket {
        requests: vec![ItemStackRequestPacketDataRequestData {
            client_request_id: TypedClientNetIdStructItemStackRequestIdTagInt32T0 {
                id: request_id,
            },
            actions: vec![action],
            strings_to_filter: Vec::new(),
            strings_to_filter_origin:
                ItemStackRequestPacketDataRequestDataStringsToFilterOrigin::Unknown,
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
            dynamic_id: None,
        },
        slot: slot.slot,
        net_id_variant: slot.stack_network_id,
    })
}
