//! Requests only a specific screen sends: enchant-option selection, beacon
//! payment and taking a server-previewed result. The request shapes follow the
//! screens' documented action families; the exact per-screen action mix still
//! needs capture verification against a vanilla server.

use std::sync::Arc;

use protocol::{CraftResult, RecipeOutput, StackRequestAction, WindowKind};
use sha2::{Digest, Sha256};

use super::cells::{Cell, Held};
use super::gesture::Submission;
use super::helpers::request_slot;
use super::overlay::DeltaGroup;
use super::{InventoryGestureError, PlayerInventoryLedger};

const ENCHANT_INPUT_SLOT: u8 = 14;
const BEACON_PAYMENT_SLOT: u8 = 27;

/// The craft action a screen's result take is announced with.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ScreenCraft {
    /// Anvil result; `rename` is the typed item name, if any.
    Anvil {
        rename: Option<Arc<str>>,
        multi_recipe_id: u32,
    },
    Grindstone {
        recipe_network_id: i32,
        repair_cost: i32,
    },
    Loom {
        pattern: Arc<str>,
    },
    /// A recipe-selected screen: stonecutter, smithing table or cartography table.
    Recipe {
        recipe_network_id: u32,
    },
    /// A recipe-selected screen whose output the client predicts from the recipe.
    Predicted {
        recipe_network_id: u32,
        output: RecipeOutput,
    },
}

impl ScreenCraft {
    const fn kinds(&self) -> &'static [WindowKind] {
        match self {
            Self::Anvil { .. } => &[WindowKind::Anvil],
            Self::Grindstone { .. } => &[WindowKind::Grindstone],
            Self::Loom { .. } => &[WindowKind::Loom],
            Self::Recipe { .. } | Self::Predicted { .. } => &[
                WindowKind::Stonecutter,
                WindowKind::Smithing,
                WindowKind::Cartography,
            ],
        }
    }
}

/// The UI input slots one screen consumes when its result is taken.
const fn input_slots(kind: WindowKind) -> &'static [u8] {
    match kind {
        WindowKind::Anvil => &[1, 2],
        WindowKind::Grindstone => &[16, 17],
        WindowKind::Loom => &[9, 10, 11],
        WindowKind::Stonecutter => &[3],
        WindowKind::Smithing => &[51, 52, 53],
        WindowKind::Cartography => &[12, 13],
        _ => &[],
    }
}

/// Items one result take consumes from an input slot (provisional).
const fn consume_amount(slot: u8, count: u16) -> u16 {
    match slot {
        // The anvil target and both grindstone inputs are used up.
        1 | 16 | 17 => count,
        // The loom pattern item stays.
        11 => 0,
        _ => 1,
    }
}

impl PlayerInventoryLedger {
    /// Selects an offered enchanting option for the input item. The server
    /// charges levels and lapis and restates the cells, so nothing is predicted.
    pub fn begin_enchant(&mut self, option_network_id: u32) -> Result<i32, InventoryGestureError> {
        let personal_generation = self.gesture_preflight(true)?;
        if self.window_kind() != Some(WindowKind::Enchanting) {
            return Err(InventoryGestureError::InvalidRequest);
        }
        self.check_surfaces([Cell::Craft(ENCHANT_INPUT_SLOT), Cell::CreatedOutput])?;
        let offered = self
            .enchant_options()
            .is_some_and(|options| options.iter().any(|o| o.network_id == option_network_id));
        if !offered || self.view().get(Cell::Craft(ENCHANT_INPUT_SLOT)).is_none() {
            return Err(InventoryGestureError::InvalidRequest);
        }
        self.submit(Submission {
            actions: vec![
                StackRequestAction::CraftRecipe {
                    recipe_network_id: option_network_id,
                    crafts: 1,
                },
                StackRequestAction::CraftResultsDeprecated {
                    results: Arc::from([]),
                    crafts: 1,
                },
            ],
            groups: Vec::new(),
            personal_generation,
            requires_distinct_stack_ids: false,
            registry_bound_merge: false,
        })
    }

    /// Pays the beacon's payment item for the chosen effects (`0` for none).
    pub fn begin_beacon_payment(
        &mut self,
        primary: i32,
        secondary: i32,
    ) -> Result<i32, InventoryGestureError> {
        let personal_generation = self.gesture_preflight(true)?;
        if self.window_kind() != Some(WindowKind::Beacon) {
            return Err(InventoryGestureError::InvalidRequest);
        }
        let cell = Cell::Craft(BEACON_PAYMENT_SLOT);
        self.check_surfaces([cell])?;
        let held = self
            .named(self.view().get(cell).cloned())?
            .ok_or(InventoryGestureError::EmptyGesture)?;
        let id = held.stack.stack_network_id;
        self.submit(Submission {
            actions: vec![
                StackRequestAction::BeaconPayment {
                    primary_effect: primary,
                    secondary_effect: secondary,
                },
                // The client follows the payment by destroying the paid item.
                StackRequestAction::Destroy {
                    amount: 1,
                    source: request_slot(cell, id, None)?,
                },
            ],
            groups: vec![DeltaGroup::Shrink {
                source: cell,
                amount: 1,
                source_id: id,
            }],
            personal_generation,
            requires_distinct_stack_ids: false,
            registry_bound_merge: false,
        })
    }

    /// Takes the result the server previewed for the open screen into an empty cursor.
    pub fn begin_screen_output(
        &mut self,
        craft: &ScreenCraft,
    ) -> Result<i32, InventoryGestureError> {
        let personal_generation = self.gesture_preflight(true)?;
        let kind = self
            .window_kind()
            .filter(|kind| craft.kinds().contains(kind))
            .ok_or(InventoryGestureError::InvalidRequest)?;
        self.check_surfaces([Cell::Cursor, Cell::CreatedOutput])?;
        if self.view().get(Cell::Cursor).is_some() {
            return Err(InventoryGestureError::InvalidRequest);
        }
        let request_id = self.peek_request_id()?;
        let predicted_distinct = matches!(
            craft,
            ScreenCraft::Predicted { .. } | ScreenCraft::Loom { .. }
        );
        let (preview, predicted_set) = match craft {
            ScreenCraft::Predicted { output, .. } => {
                if self.negotiated_item_entry(output.network_id).is_none() {
                    return Err(InventoryGestureError::InvalidRequest);
                }
                let user_data: Arc<[u8]> = if output.empty_envelope {
                    Arc::from([0; 10])
                } else {
                    Arc::from([])
                };
                let stack = protocol::NetworkItemStack {
                    network_id: output.network_id,
                    metadata: u32::from(output.aux),
                    stack_network_id: request_id,
                    count: u16::from(output.count),
                    nbt_digest: Sha256::digest(&user_data).into(),
                    block_runtime_id: i32::try_from(output.block_runtime_id)
                        .map_err(|_| InventoryGestureError::InvalidRequest)?,
                    extra_data: user_data,
                };
                let held = Held {
                    stack,
                    overlay: None,
                };
                (held.clone(), Some(held))
            }
            // The loom's patterned banner has no client-side form; the copy
            // stands in until the server restates the cursor.
            ScreenCraft::Loom { .. } => {
                let banner = self
                    .view()
                    .get(Cell::Craft(9))
                    .cloned()
                    .ok_or(InventoryGestureError::EmptyGesture)?;
                if banner.stack.stack_network_id <= 0 {
                    return Err(InventoryGestureError::AwaitingIdentity);
                }
                let mut stack = banner.stack;
                stack.count = 1;
                stack.stack_network_id = request_id;
                stack.extra_data = Arc::from([]);
                stack.nbt_digest = Sha256::digest([]).into();
                let held = Held {
                    stack,
                    overlay: None,
                };
                (held.clone(), Some(held))
            }
            _ => {
                let preview = self
                    .view()
                    .get(Cell::CreatedOutput)
                    .cloned()
                    .ok_or(InventoryGestureError::EmptyGesture)?;
                if preview.stack.stack_network_id <= 0 {
                    return Err(InventoryGestureError::AwaitingIdentity);
                }
                (preview, None)
            }
        };
        let entry = self
            .negotiated_item_entry(preview.stack.network_id)
            .ok_or(InventoryGestureError::InvalidRequest)?;
        let results = Arc::from([CraftResult {
            identifier: Arc::clone(&entry.identifier),
            aux: i32::try_from(preview.stack.metadata).unwrap_or(0),
            count: preview.stack.count,
            block_runtime_id: u32::try_from(preview.stack.block_runtime_id).unwrap_or(0),
            user_data: Arc::clone(&preview.stack.extra_data),
        }]);
        let (first, filter_strings) = match craft {
            ScreenCraft::Anvil {
                rename,
                multi_recipe_id,
            } => (
                StackRequestAction::CraftRecipeOptional {
                    recipe_network_id: *multi_recipe_id,
                    filtered_string_index: if rename.is_some() { 0 } else { -1 },
                },
                rename
                    .iter()
                    .map(|name| name.to_string())
                    .collect::<Vec<String>>(),
            ),
            ScreenCraft::Grindstone {
                recipe_network_id,
                repair_cost,
            } => (
                StackRequestAction::Grindstone {
                    recipe_network_id: *recipe_network_id,
                    crafts: 1,
                    repair_cost: *repair_cost,
                },
                Vec::new(),
            ),
            ScreenCraft::Loom { pattern } => (
                StackRequestAction::Loom {
                    pattern: Arc::clone(pattern),
                    crafts: 1,
                },
                Vec::new(),
            ),
            ScreenCraft::Recipe { recipe_network_id }
            | ScreenCraft::Predicted {
                recipe_network_id, ..
            } => (
                StackRequestAction::CraftRecipe {
                    recipe_network_id: *recipe_network_id,
                    crafts: 1,
                },
                Vec::new(),
            ),
        };
        let mut actions = vec![
            first,
            StackRequestAction::CraftResultsDeprecated { results, crafts: 1 },
        ];
        let mut groups = Vec::new();
        for slot in input_slots(kind) {
            let cell = Cell::Craft(*slot);
            let Some(held) = self.view().get(cell) else {
                continue;
            };
            let amount = consume_amount(*slot, held.stack.count).min(held.stack.count);
            if amount == 0 {
                continue;
            }
            if self.awaiting_identity(held) {
                return Err(InventoryGestureError::AwaitingIdentity);
            }
            let id = held.stack.stack_network_id;
            actions.push(StackRequestAction::Consume {
                amount: u8::try_from(amount).map_err(|_| InventoryGestureError::InvalidRequest)?,
                source: request_slot(cell, id, None)?,
            });
            groups.push(DeltaGroup::Shrink {
                source: cell,
                amount,
                source_id: id,
            });
        }
        let take =
            u8::try_from(preview.stack.count).map_err(|_| InventoryGestureError::InvalidRequest)?;
        actions.push(StackRequestAction::Take {
            amount: take,
            source: request_slot(Cell::CreatedOutput, preview.stack.stack_network_id, None)?,
            destination: request_slot(Cell::Cursor, 0, None)?,
        });
        if let Some(held) = predicted_set {
            groups.push(DeltaGroup::Set {
                cell: Cell::CreatedOutput,
                held,
            });
        }
        groups.push(DeltaGroup::Transfer {
            source: Cell::CreatedOutput,
            destination: Cell::Cursor,
            amount: preview.stack.count,
            source_id: preview.stack.stack_network_id,
            destination_id: None,
            capacity: None,
        });
        self.submit_filtered(
            Submission {
                actions,
                groups,
                personal_generation,
                requires_distinct_stack_ids: predicted_distinct,
                registry_bound_merge: false,
            },
            filter_strings,
        )
    }
}
