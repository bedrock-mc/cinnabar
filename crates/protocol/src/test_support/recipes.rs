//! CraftingData fixtures admitted through the real recipe decoder, and crafting request observations.

use bedrock_protocol_1_26_51::enums::RecipeUnlockingRequirementUnlockingContext as UnlockingContext;
use bedrock_protocol_1_26_51::packets::{self, CraftingData};
use bedrock_protocol_1_26_51::types::{
    ContainerMixDataEntry, ItemDescriptor, NetworkItemInstanceDescriptorSerializedData,
    PotionMixDataEntry, RecipeIngredientSerializedData, RecipeNetID,
    RecipeUnlockRequirementSerializedData, ShapedRecipe, ShapelessRecipe,
    StackRequestAction as Action, StackRequestSlotInfo,
};
use bedrock_protocol_1_26_51::wire::{self, Encode};
use bytes::Bytes;

use crate::{Packet, RecipeUpdate, decode_recipe_update};

/// One recipe ingredient cell; an empty `descriptor` is an empty cell.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IngredientFixture {
    /// `(key, value)` pairs, such as `("name", "minecraft:stick")` or `("item_tag", ...)`.
    pub descriptor: Vec<(String, String)>,
    pub aux: i32,
    pub count: i32,
}

impl IngredientFixture {
    /// One item by name with the given metadata.
    pub fn named(name: &str, aux: i32) -> Self {
        Self {
            descriptor: vec![("name".into(), name.into())],
            aux,
            count: 1,
        }
    }

    /// One member of an item tag.
    pub fn tagged(tag: &str) -> Self {
        Self {
            descriptor: vec![("item_tag".into(), tag.into())],
            aux: 0,
            count: 1,
        }
    }

    /// An empty cell, which counts zero items.
    pub fn empty() -> Self {
        Self::default()
    }

    fn wire(&self) -> RecipeIngredientSerializedData {
        RecipeIngredientSerializedData {
            descriptor: self
                .descriptor
                .iter()
                .map(|(key, value)| (key.as_str().into(), value.as_str().into()))
                .collect(),
            aux_value: wire::ZigZag32(self.aux),
            stack_size: wire::ZigZag32(self.count),
        }
    }
}

/// One recipe output by item network ID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputFixture {
    pub network_id: i32,
    pub count: u16,
    pub metadata: u32,
    pub block_runtime_id: i32,
    pub user_data: Vec<u8>,
}

impl OutputFixture {
    /// A plain output with no metadata, block or user data.
    pub fn new(network_id: i32, count: u16) -> Self {
        Self {
            network_id,
            count,
            metadata: 0,
            block_runtime_id: 0,
            user_data: Vec::new(),
        }
    }

    fn wire(&self) -> NetworkItemInstanceDescriptorSerializedData {
        NetworkItemInstanceDescriptorSerializedData {
            id: wire::ZigZag32(self.network_id),
            stack_size: wire::U16LE(self.count),
            aux_value: wire::VarUInt(self.metadata),
            block_runtime_id: wire::ZigZag32(self.block_runtime_id),
            user_data_buffer: Bytes::copy_from_slice(&self.user_data),
        }
    }
}

/// A recipe's discovery requirement; `context` is its wire value.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UnlockingFixture {
    pub context: i32,
    pub ingredients: Option<Vec<IngredientFixture>>,
}

impl UnlockingFixture {
    fn wire(&self) -> RecipeUnlockRequirementSerializedData {
        RecipeUnlockRequirementSerializedData {
            unlocking_context: match self.context {
                0 => UnlockingContext::None,
                1 => UnlockingContext::Alwaysunlocked,
                2 => UnlockingContext::Playerinwater,
                3 => UnlockingContext::Playerhasmanyitems,
                other => UnlockingContext::Unknown(other),
            },
            unlocking_ingredients: self
                .ingredients
                .as_ref()
                .map(|ingredients| ingredients.iter().map(IngredientFixture::wire).collect()),
        }
    }
}

/// One shaped or shapeless recipe.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecipeFixture {
    pub recipe_id: String,
    /// `(width, height)` for a shaped recipe; `None` for a shapeless one.
    pub shape: Option<(i32, i32)>,
    /// Shaped only: whether the horizontal mirror also matches.
    pub assume_symmetry: bool,
    pub ingredients: Vec<IngredientFixture>,
    pub results: Vec<OutputFixture>,
    /// The station tag, such as `"crafting_table"` or `"furnace"`.
    pub tag: String,
    pub priority: i32,
    pub net_id: u32,
    pub unlocking: Option<UnlockingFixture>,
}

impl RecipeFixture {
    /// A shaped `crafting_table` recipe at priority 0 with net ID 0.
    pub fn shaped(
        recipe_id: &str,
        width: i32,
        height: i32,
        ingredients: Vec<IngredientFixture>,
        results: Vec<OutputFixture>,
    ) -> Self {
        Self {
            shape: Some((width, height)),
            ..Self::shapeless(recipe_id, ingredients, results)
        }
    }

    /// A shapeless `crafting_table` recipe at priority 0 with net ID 0.
    pub fn shapeless(
        recipe_id: &str,
        ingredients: Vec<IngredientFixture>,
        results: Vec<OutputFixture>,
    ) -> Self {
        Self {
            recipe_id: recipe_id.into(),
            shape: None,
            assume_symmetry: false,
            ingredients,
            results,
            tag: "crafting_table".into(),
            priority: 0,
            net_id: 0,
            unlocking: None,
        }
    }
}

/// A CraftingData packet; shaped and shapeless recipes each keep their relative order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CraftingDataFixture {
    pub recipes: Vec<RecipeFixture>,
    pub clear_recipes: bool,
    /// Potion mixes: input id and aux, reagent id and aux, output id and aux.
    pub potions: Vec<[i32; 6]>,
    /// Container mixes: input, reagent and output ids.
    pub containers: Vec<[i32; 3]>,
}

impl CraftingDataFixture {
    /// A clearing update that carries exactly `recipes`.
    pub fn clearing(recipes: Vec<RecipeFixture>) -> Self {
        Self {
            recipes,
            clear_recipes: true,
            ..Self::default()
        }
    }

    /// The packet body `decode_recipe_update` reads.
    pub fn encode(&self) -> Vec<u8> {
        let mut packet = CraftingData {
            clear_recipes: self.clear_recipes,
            ..Default::default()
        };
        for recipe in &self.recipes {
            let ingredients = recipe.ingredients.iter().map(IngredientFixture::wire);
            let results = recipe.results.iter().map(OutputFixture::wire).collect();
            let unlocking_requirement = recipe.unlocking.as_ref().map(UnlockingFixture::wire);
            let net_id = RecipeNetID(recipe.net_id);
            match recipe.shape {
                Some((width, height)) => packet.shaped_recipes.push(ShapedRecipe {
                    recipe_id: recipe.recipe_id.as_str().into(),
                    width: wire::ZigZag32(width),
                    height: wire::ZigZag32(height),
                    ingredients: ingredients.collect(),
                    results,
                    tag: recipe.tag.as_str().into(),
                    priority: wire::ZigZag32(recipe.priority),
                    assume_symmetry: recipe.assume_symmetry,
                    unlocking_requirement,
                    net_id,
                    ..Default::default()
                }),
                None => packet.shapeless_recipes.push(ShapelessRecipe {
                    recipe_id: recipe.recipe_id.as_str().into(),
                    ingredients: ingredients.collect(),
                    results,
                    tag: recipe.tag.as_str().into(),
                    priority: wire::ZigZag32(recipe.priority),
                    unlocking_requirement,
                    net_id,
                    ..Default::default()
                }),
            }
        }
        let z = wire::ZigZag32;
        packet.potion_mixes = self
            .potions
            .iter()
            .map(
                |[input, input_aux, reagent, reagent_aux, output, output_aux]| PotionMixDataEntry {
                    from_potion_id: z(*input),
                    from_item_aux: z(*input_aux),
                    reagent_item_id: z(*reagent),
                    reagent_item_aux: z(*reagent_aux),
                    to_potion_id: z(*output),
                    to_item_aux: z(*output_aux),
                },
            )
            .collect();
        packet.container_mixes = self
            .containers
            .iter()
            .map(|[input, reagent, output]| ContainerMixDataEntry {
                from_item_id: z(*input),
                reagent_item_id: z(*reagent),
                to_item_id: z(*output),
            })
            .collect();
        packet
            .encode_to_vec()
            .expect("fixture CraftingData encodes")
    }

    /// The update the real decoder admits from [`Self::encode`]; panics on a decode error.
    pub fn update(&self) -> RecipeUpdate {
        decode_recipe_update(&self.encode()).expect("fixture CraftingData decodes")
    }
}

/// Admits the first packet of an uncompressed batch as a CraftingData update; panics on bad wire.
pub fn recipe_update_from_batch(batch: &'static [u8]) -> RecipeUpdate {
    let raw = crate::raw_packet::decode_batch(Bytes::from_static(batch), 4096)
        .expect("fixture batch frames")
        .remove(0);
    decode_recipe_update(raw.body()).expect("fixture CraftingData decodes")
}

/// `packet` with its subclient header set, for comparing against captured wire.
pub fn with_subclients(mut packet: Packet, from_subclient: u32, to_subclient: u32) -> Packet {
    packet.header.from_subclient = from_subclient;
    packet.header.to_subclient = to_subclient;
    packet
}

/// A slot an outbound stack request action names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StackSlotObservation {
    pub slot: u8,
    pub stack_network_id: i32,
}

impl StackSlotObservation {
    fn from_wire(slot: &StackRequestSlotInfo) -> Self {
        Self {
            slot: slot.slot.0,
            stack_network_id: slot.net_id_variant.0,
        }
    }
}

/// One outbound stack request action, reduced to what crafting tests assert.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StackActionObservation {
    CraftRecipe {
        recipe_network_id: u32,
    },
    /// Each advertised result's item name; other descriptor kinds read as `None`.
    CraftResults(Vec<Option<String>>),
    Consume {
        amount: u8,
        source: StackSlotObservation,
    },
    Take {
        amount: u8,
        source: StackSlotObservation,
        destination: StackSlotObservation,
    },
    Other,
}

/// The first request's actions in an ItemStackRequest packet; `None` for any other packet.
pub fn stack_request_actions(packet: &Packet) -> Option<Vec<StackActionObservation>> {
    let packets::Packet::ItemStackRequest(request) = &*packet.data else {
        return None;
    };
    let actions = request
        .requests
        .first()?
        .actions
        .iter()
        .map(|action| match action {
            Action::CraftRecipeActionData { recipe_net_id, .. } => {
                StackActionObservation::CraftRecipe {
                    recipe_network_id: recipe_net_id.0,
                }
            }
            Action::CraftResultsActionData { craft_results, .. } => {
                StackActionObservation::CraftResults(
                    craft_results
                        .iter()
                        .map(|result| match &result.item_descriptor {
                            ItemDescriptor::ItemNameDescriptorData { full_name, .. } => {
                                Some(full_name.to_string_lossy().into_owned())
                            }
                            _ => None,
                        })
                        .collect(),
                )
            }
            Action::ConsumeActionData { amount, source, .. } => StackActionObservation::Consume {
                amount: amount.0,
                source: StackSlotObservation::from_wire(source),
            },
            Action::TakeActionData {
                amount,
                source,
                destination,
                ..
            } => StackActionObservation::Take {
                amount: amount.0,
                source: StackSlotObservation::from_wire(source),
                destination: StackSlotObservation::from_wire(destination),
            },
            _ => StackActionObservation::Other,
        });
    Some(actions.collect())
}

/// Decodes a generated unlock packet through the public world inventory normalizer.
pub fn recipe_unlock_event(operation: u32, names: &[&str]) -> crate::InventoryEvent {
    let packet = crate::Packet::new(packets::UnlockedRecipes {
        packet_type: operation.into(),
        unlocked_recipes_list: names.iter().map(|name| (*name).into()).collect(),
    });
    let decoded = crate::decode_batch(crate::encode(&packet).expect("unlock fixture encoding"))
        .expect("unlock fixture decoding")
        .pop()
        .expect("one unlock packet");
    let Some(crate::WorldEvent::Inventory(event)) =
        crate::into_world_event(decoded, 0).expect("unlock fixture normalization")
    else {
        panic!("unlock inventory event")
    };
    event
}
