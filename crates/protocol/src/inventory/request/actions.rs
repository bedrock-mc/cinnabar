//! Typed item stack request actions and their exact wire slot addressing.

use std::sync::Arc;

use valentine::bedrock::version::v1_26_51::{
    EnumsContainerEnumName as Name, EnumsItemStackRequestActionType as Kind,
    EnumsItemStackRequestCerealItemDescriptorType as DescriptorKind, FullContainerName,
    ItemStackRequestCerealBeaconPaymentActionData, ItemStackRequestCerealConsumeActionData,
    ItemStackRequestCerealCraftCreativeActionData, ItemStackRequestCerealCraftLoomActionData,
    ItemStackRequestCerealCraftRecipeActionData, ItemStackRequestCerealCraftRecipeAutoActionData,
    ItemStackRequestCerealCraftRecipeOptionalActionData,
    ItemStackRequestCerealCraftRepairAndDisenchantActionData,
    ItemStackRequestCerealCraftResultsActionData, ItemStackRequestCerealCreateActionData,
    ItemStackRequestCerealDestroyActionData, ItemStackRequestCerealDropActionData,
    ItemStackRequestCerealEmptyItemDescriptorData, ItemStackRequestCerealItemNameDescriptorData,
    ItemStackRequestCerealItemTagDescriptorData, ItemStackRequestCerealMineBlockActionData,
    ItemStackRequestCerealNetworkItemInstanceDescriptorData,
    ItemStackRequestCerealNetworkItemInstanceDescriptorDataItemDescriptor as ResultDescriptor,
    ItemStackRequestCerealPlaceActionData, ItemStackRequestCerealRecipeIngredientData,
    ItemStackRequestCerealRecipeIngredientDataItemDescriptor as IngredientDescriptor,
    ItemStackRequestCerealSlotInfoData, ItemStackRequestCerealSwapActionData,
    ItemStackRequestCerealTakeActionData, ItemStackRequestPacketDataRequestDataActionsItem as Item,
    TypedServerNetIdstructRecipeNetIdTag,
};

use super::super::InventoryPacketError;
use super::super::container_policy::{ContainerWindow, container_window};

pub const PLAYER_INVENTORY_SLOTS: u8 = 36;
/// Personal and crafting-table grid cells in the UI inventory.
pub const CRAFTING_INPUT_SLOTS: std::ops::RangeInclusive<u8> = 28..=40;
pub const CREATED_OUTPUT_SLOT: u8 = 50;
/// Armor cells, including the body slot.
pub const ARMOR_SLOTS: u8 = 5;
/// The offhand's only request slot on the wire.
const OFFHAND_WIRE_SLOT: u8 = 1;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum StackRequestContainer {
    /// Player cells 0..36, named as hotbar below 9 and inventory above.
    PlayerInventory,
    Cursor,
    Armor,
    /// The single offhand cell; always addressed as wire slot 1.
    Offhand,
    CraftingInput,
    CreatedOutput,
    LevelEntity {
        dynamic_id: Option<u32>,
    },
    /// Any other container name owned by the open window.
    OpenWindow {
        name: u8,
        dynamic_id: Option<u32>,
    },
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct StackRequestSlot {
    pub container: StackRequestContainer,
    pub slot: u8,
    /// A positive server id, `0` for an empty cell, or a negative odd request
    /// id naming a prior prediction (including empty cells) or created output.
    pub stack_network_id: i32,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum StackItemDescriptor {
    Empty,
    Name { identifier: Arc<str>, aux: i32 },
    Tag(Arc<str>),
}

/// One predicted output declared by a deprecated craft-results action.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CraftResult {
    pub identifier: Arc<str>,
    pub aux: i32,
    pub count: u16,
    pub block_runtime_id: u32,
    pub user_data: Arc<[u8]>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AutoCraftIngredient {
    pub descriptor: StackItemDescriptor,
    pub count: u16,
}

#[derive(Debug, Clone, Eq, PartialEq)]
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
    Drop {
        amount: u8,
        source: StackRequestSlot,
        randomly: bool,
    },
    Destroy {
        amount: u8,
        source: StackRequestSlot,
    },
    Consume {
        amount: u8,
        source: StackRequestSlot,
    },
    Create {
        results_index: u8,
    },
    BeaconPayment {
        primary_effect: i32,
        secondary_effect: i32,
    },
    MineBlock {
        hotbar_slot: i32,
        predicted_durability: i32,
        stack_network_id: i32,
    },
    CraftRecipe {
        recipe_network_id: u32,
        crafts: u8,
    },
    AutoCraft {
        recipe_network_id: u32,
        crafts: u8,
        ingredients: Arc<[AutoCraftIngredient]>,
    },
    CraftCreative {
        creative_item_network_id: u32,
        crafts: u8,
    },
    CraftRecipeOptional {
        recipe_network_id: u32,
        filtered_string_index: i32,
    },
    /// The grindstone's repair-and-disenchant craft.
    Grindstone {
        recipe_network_id: i32,
        crafts: u8,
        repair_cost: i32,
    },
    Loom {
        pattern: Arc<str>,
        crafts: u8,
    },
    CraftResultsDeprecated {
        results: Arc<[CraftResult]>,
        crafts: u8,
    },
}

fn amount(amount: u8) -> Result<u8, InventoryPacketError> {
    if amount == 0 {
        return Err(InventoryPacketError::InvalidStackRequestAmount);
    }
    Ok(amount)
}

fn crafts(crafts: u8) -> Result<u8, InventoryPacketError> {
    if crafts == 0 {
        return Err(InventoryPacketError::InvalidStackRequestAmount);
    }
    Ok(crafts)
}

pub(super) fn encode(action: &StackRequestAction) -> Result<Item, InventoryPacketError> {
    use StackRequestAction as A;
    Ok(match action {
        A::Take {
            amount: count,
            source,
            destination,
        } => Item::TakeActionData(Box::new(ItemStackRequestCerealTakeActionData {
            actiontype: Kind::Take,
            amount: amount(*count)?,
            source: slot(*source)?,
            destination: slot(*destination)?,
        })),
        A::Place {
            amount: count,
            source,
            destination,
        } => Item::PlaceActionData(Box::new(ItemStackRequestCerealPlaceActionData {
            actiontype: Kind::Place,
            amount: amount(*count)?,
            source: slot(*source)?,
            destination: slot(*destination)?,
        })),
        A::Swap {
            source,
            destination,
        } => Item::SwapActionData(ItemStackRequestCerealSwapActionData {
            actiontype: Kind::Swap,
            source: slot(*source)?,
            destination: slot(*destination)?,
        }),
        A::Drop {
            amount: count,
            source,
            randomly,
        } => Item::DropActionData(Box::new(ItemStackRequestCerealDropActionData {
            actiontype: Kind::Drop,
            amount: amount(*count)?,
            source: slot(*source)?,
            randomly: *randomly,
        })),
        A::Destroy {
            amount: count,
            source,
        } => Item::DestroyActionData(ItemStackRequestCerealDestroyActionData {
            actiontype: Kind::Destroy,
            amount: amount(*count)?,
            source: slot(*source)?,
        }),
        A::Consume {
            amount: count,
            source,
        } => Item::ConsumeActionData(ItemStackRequestCerealConsumeActionData {
            actiontype: Kind::Consume,
            amount: amount(*count)?,
            source: slot(*source)?,
        }),
        A::Create { results_index } => {
            Item::CreateActionData(ItemStackRequestCerealCreateActionData {
                actiontype: Kind::Create,
                results_index: *results_index,
            })
        }
        A::BeaconPayment {
            primary_effect,
            secondary_effect,
        } => Item::BeaconPaymentActionData(ItemStackRequestCerealBeaconPaymentActionData {
            actiontype: Kind::Screenbeaconpayment,
            primary_effect_id: *primary_effect,
            secondary_effect_id: *secondary_effect,
        }),
        A::MineBlock {
            hotbar_slot,
            predicted_durability,
            stack_network_id,
        } => Item::MineBlockActionData(Box::new(ItemStackRequestCerealMineBlockActionData {
            actiontype: Kind::Screenhudmineblock,
            slot: *hotbar_slot,
            predicted_durability: *predicted_durability,
            net_id_variant: *stack_network_id,
        })),
        A::CraftRecipe {
            recipe_network_id,
            crafts: count,
        } => Item::CraftRecipeActionData(ItemStackRequestCerealCraftRecipeActionData {
            actiontype: Kind::Craftrecipe,
            recipe_net_id: recipe(*recipe_network_id),
            numberofrequestedcrafts: crafts(*count)?,
        }),
        A::AutoCraft {
            recipe_network_id,
            crafts: count,
            ingredients,
        } => Item::CraftRecipeAutoActionData(Box::new(
            ItemStackRequestCerealCraftRecipeAutoActionData {
                actiontype: Kind::Craftrecipeauto,
                recipe_net_id: recipe(*recipe_network_id),
                numberofrequestedcrafts: crafts(*count)?,
                ingredients: ingredients
                    .iter()
                    .map(|ingredient| ItemStackRequestCerealRecipeIngredientData {
                        item_descriptor: ingredient_descriptor(&ingredient.descriptor),
                        stack_size: ingredient.count,
                    })
                    .collect(),
            },
        )),
        A::CraftCreative {
            creative_item_network_id,
            crafts: count,
        } => Item::CraftCreativeActionData(ItemStackRequestCerealCraftCreativeActionData {
            actiontype: Kind::Craftcreative,
            creative_item_net_id: *creative_item_network_id,
            numberofrequestedcrafts: crafts(*count)?,
        }),
        A::CraftRecipeOptional {
            recipe_network_id,
            filtered_string_index,
        } => Item::CraftRecipeOptionalActionData(
            ItemStackRequestCerealCraftRecipeOptionalActionData {
                actiontype: Kind::Craftrecipeoptional,
                recipe_net_id: recipe(*recipe_network_id),
                filtered_string_index: *filtered_string_index,
            },
        ),
        A::Grindstone {
            recipe_network_id,
            crafts: count,
            repair_cost,
        } => Item::CraftRepairAndDisenchantActionData(Box::new(
            ItemStackRequestCerealCraftRepairAndDisenchantActionData {
                actiontype: Kind::Craftrepairanddisenchant,
                recipe_net_id: *recipe_network_id,
                numberofrequestedcrafts: crafts(*count)?,
                repair_cost: *repair_cost,
            },
        )),
        A::Loom {
            pattern,
            crafts: count,
        } => Item::CraftLoomActionData(ItemStackRequestCerealCraftLoomActionData {
            actiontype: Kind::Craftloom,
            pattern_name_id: pattern.to_string(),
            num_crafts: crafts(*count)?,
        }),
        A::CraftResultsDeprecated {
            results,
            crafts: count,
        } => Item::CraftResultsActionData(ItemStackRequestCerealCraftResultsActionData {
            actiontype: Kind::Craftresults,
            craft_results: results.iter().map(craft_result).collect(),
            num_crafts: crafts(*count)?,
        }),
    })
}

const fn recipe(raw_id: u32) -> TypedServerNetIdstructRecipeNetIdTag {
    TypedServerNetIdstructRecipeNetIdTag { raw_id }
}

fn craft_result(result: &CraftResult) -> ItemStackRequestCerealNetworkItemInstanceDescriptorData {
    ItemStackRequestCerealNetworkItemInstanceDescriptorData {
        item_descriptor: ResultDescriptor::ItemNameDescriptorData(
            ItemStackRequestCerealItemNameDescriptorData {
                descriptor_type: DescriptorKind::Itemname,
                full_name: result.identifier.to_string(),
                aux_value: result.aux,
            },
        ),
        stacksize: result.count,
        block_runtime_id: result.block_runtime_id,
        user_data_buffer: result.user_data.to_vec(),
    }
}

fn ingredient_descriptor(descriptor: &StackItemDescriptor) -> IngredientDescriptor {
    match descriptor {
        StackItemDescriptor::Empty => IngredientDescriptor::EmptyItemDescriptorData(
            ItemStackRequestCerealEmptyItemDescriptorData {
                descriptor_type: DescriptorKind::Empty,
            },
        ),
        StackItemDescriptor::Name { identifier, aux } => {
            IngredientDescriptor::ItemNameDescriptorData(
                ItemStackRequestCerealItemNameDescriptorData {
                    descriptor_type: DescriptorKind::Itemname,
                    full_name: identifier.to_string(),
                    aux_value: *aux,
                },
            )
        }
        StackItemDescriptor::Tag(tag) => IngredientDescriptor::ItemTagDescriptorData(
            ItemStackRequestCerealItemTagDescriptorData {
                descriptor_type: DescriptorKind::Itemtag,
                item_tag: tag.to_string(),
            },
        ),
    }
}

/// Encodes one slot with the container name and wire index vanilla uses.
fn slot(
    slot: StackRequestSlot,
) -> Result<ItemStackRequestCerealSlotInfoData, InventoryPacketError> {
    use StackRequestContainer as C;
    let invalid = InventoryPacketError::InvalidStackRequestSlot {
        container: slot.container,
        slot: slot.slot,
    };
    let (container_name, wire_slot, dynamic_id) = match slot.container {
        C::PlayerInventory if slot.slot < crate::HOTBAR_SLOT_COUNT => {
            (Name::Hotbarcontainer, slot.slot, None)
        }
        C::PlayerInventory if slot.slot < PLAYER_INVENTORY_SLOTS => {
            (Name::Inventorycontainer, slot.slot, None)
        }
        C::Cursor if slot.slot == 0 => (Name::Cursorcontainer, 0, None),
        C::Armor if slot.slot < ARMOR_SLOTS => (Name::Armorcontainer, slot.slot, None),
        C::Offhand if matches!(slot.slot, 0 | OFFHAND_WIRE_SLOT) => {
            (Name::Offhandcontainer, OFFHAND_WIRE_SLOT, None)
        }
        C::CraftingInput if CRAFTING_INPUT_SLOTS.contains(&slot.slot) => {
            (Name::Craftinginputcontainer, slot.slot, None)
        }
        C::CreatedOutput if slot.slot == CREATED_OUTPUT_SLOT => {
            (Name::Createdoutputcontainer, slot.slot, None)
        }
        C::LevelEntity { dynamic_id } => (Name::Levelentitycontainer, slot.slot, dynamic_id),
        C::OpenWindow { name, dynamic_id }
            if container_window(name) == Some(ContainerWindow::Open) =>
        {
            (
                open_window_name(name).ok_or(invalid.clone())?,
                slot.slot,
                dynamic_id,
            )
        }
        _ => return Err(invalid),
    };
    Ok(ItemStackRequestCerealSlotInfoData {
        fullcontainername: FullContainerName {
            container_name,
            dynamic_id,
        },
        slot: wire_slot,
        net_id_variant: slot.stack_network_id,
    })
}

fn open_window_name(code: u8) -> Option<Name> {
    use valentine::bedrock::codec::BedrockCodec;
    let name = Name::decode(&mut bytes::Bytes::copy_from_slice(&[code]), ()).ok()?;
    (!matches!(name, Name::Unknown(_))).then_some(name)
}

#[cfg(test)]
mod tests {
    use bytes::BytesMut;
    use valentine::bedrock::codec::BedrockCodec;
    use valentine::bedrock::version::v1_26_51::{ItemStackRequestPacket, McpePacketData};

    use super::super::item_stack_request_packet;
    use super::*;

    fn at(container: StackRequestContainer, slot: u8, id: i32) -> StackRequestSlot {
        StackRequestSlot {
            container,
            slot,
            stack_network_id: id,
        }
    }

    fn every_action() -> Vec<StackRequestAction> {
        use StackRequestAction as A;
        use StackRequestContainer as C;
        let player = at(C::PlayerInventory, 12, 7);
        let cursor = at(C::Cursor, 0, 0);
        vec![
            A::CraftRecipe {
                recipe_network_id: 9,
                crafts: 2,
            },
            A::CraftResultsDeprecated {
                results: Arc::from([CraftResult {
                    identifier: Arc::from("minecraft:stick"),
                    aux: 0,
                    count: 8,
                    block_runtime_id: 0,
                    user_data: Arc::from([]),
                }]),
                crafts: 2,
            },
            A::Consume {
                amount: 2,
                source: at(C::CraftingInput, 28, 11),
            },
            A::Take {
                amount: 8,
                source: at(C::CreatedOutput, 50, -3),
                destination: cursor,
            },
            A::Place {
                amount: 1,
                source: player,
                destination: at(C::Offhand, 0, 0),
            },
            A::Swap {
                source: at(C::Armor, 4, 3),
                destination: player,
            },
            A::Drop {
                amount: 1,
                source: player,
                randomly: false,
            },
            A::Destroy {
                amount: 1,
                source: at(
                    C::LevelEntity {
                        dynamic_id: Some(4),
                    },
                    2,
                    5,
                ),
            },
            A::Create { results_index: 0 },
            A::BeaconPayment {
                primary_effect: 1,
                secondary_effect: 10,
            },
            A::MineBlock {
                hotbar_slot: 2,
                predicted_durability: 30,
                stack_network_id: 7,
            },
            A::AutoCraft {
                recipe_network_id: 9,
                crafts: 1,
                ingredients: Arc::from([
                    AutoCraftIngredient {
                        descriptor: StackItemDescriptor::Tag(Arc::from("minecraft:planks")),
                        count: 1,
                    },
                    AutoCraftIngredient {
                        descriptor: StackItemDescriptor::Name {
                            identifier: Arc::from("minecraft:stick"),
                            aux: 0,
                        },
                        count: 2,
                    },
                    AutoCraftIngredient {
                        descriptor: StackItemDescriptor::Empty,
                        count: 0,
                    },
                ]),
            },
            A::CraftCreative {
                creative_item_network_id: 44,
                crafts: 1,
            },
            A::CraftRecipeOptional {
                recipe_network_id: 9,
                filtered_string_index: 0,
            },
            A::Grindstone {
                recipe_network_id: 3,
                crafts: 1,
                repair_cost: 2,
            },
            A::Loom {
                pattern: Arc::from("bo"),
                crafts: 1,
            },
            A::Place {
                amount: 1,
                source: player,
                destination: at(
                    C::OpenWindow {
                        name: 25,
                        dynamic_id: None,
                    },
                    0,
                    0,
                ),
            },
        ]
    }

    /// Every action variant survives an encode/decode round trip unchanged.
    #[test]
    fn every_action_round_trips_through_the_generated_codec() {
        let packet = item_stack_request_packet(-3, &every_action()).unwrap();
        let McpePacketData::ItemStackRequestPacket(request) = packet.data else {
            panic!("an item stack request");
        };
        assert_eq!(request.requests[0].actions.len(), every_action().len());
        let mut bytes = BytesMut::new();
        request.encode(&mut bytes).unwrap();
        let decoded = ItemStackRequestPacket::decode(&mut bytes.freeze(), ()).unwrap();
        assert_eq!(decoded, request);
    }
}
