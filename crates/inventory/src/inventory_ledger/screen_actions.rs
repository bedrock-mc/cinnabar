//! Requests only a specific screen sends: enchant-option selection, beacon
//! payment and taking a server-previewed result. The request shapes follow the
//! screens' documented action families; the exact per-screen action mix still
//! needs capture verification against a vanilla server.

use std::sync::Arc;

use protocol::{CraftResult, RecipeOutput, StackRequestAction, WindowKind};
use sha2::{Digest, Sha256};

use super::cells::{Cell, Held};
use super::crafting::CraftSink;
use super::gesture::Submission;
use super::helpers::request_slot;
use super::overlay::DeltaGroup;
use super::registry::entry_capacity;
use super::{InventoryGestureError, PlayerInventoryLedger};

const ENCHANT_INPUT_SLOT: u8 = 14;
const BEACON_PAYMENT_SLOT: u8 = 27;
/// The anvil's first input, the item it works on.
pub const ANVIL_TARGET_SLOT: u8 = 1;
/// The anvil's second input: a repair material, enchanted book or second copy.
pub const ANVIL_MATERIAL_SLOT: u8 = 2;

/// The craft action a screen's result take is announced with.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ScreenCraft {
    Trade {
        recipe_network_id: u32,
        output: protocol::NetworkItemStack,
        consumed: [u16; 2],
    },
    /// Anvil result. `rename` is the name-box text, sent as the filter string; the server reads
    /// its absence as no name, which clears a custom one. `recipe_network_id` is 0 unless a
    /// recipe drives the result. `output` supplies a prediction when no server result exists;
    /// `material_used` is its second-input consumption.
    Anvil {
        rename: Option<Arc<str>>,
        recipe_network_id: u32,
        output: Option<protocol::NetworkItemStack>,
        material_used: u16,
    },
    /// Grindstone result. `recipe_network_id` is the server stack id of the input the result
    /// copies and `repair_cost` the result's penalty; `output` supplies a prediction when the
    /// server previews no result.
    Grindstone {
        recipe_network_id: i32,
        repair_cost: i32,
        output: Option<protocol::NetworkItemStack>,
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
    /// A cartography table result predicted from its recipe. `name` is sent as the filter
    /// string; the server clears the map's custom name when it is empty.
    Cartography {
        recipe_network_id: u32,
        output: RecipeOutput,
        name: Arc<str>,
    },
}

impl ScreenCraft {
    const fn kinds(&self) -> &'static [WindowKind] {
        match self {
            Self::Trade { .. } => &[
                WindowKind::Trade { modern: true },
                WindowKind::Trade { modern: false },
            ],
            Self::Anvil { .. } => &[WindowKind::Anvil],
            Self::Grindstone { .. } => &[WindowKind::Grindstone],
            Self::Loom { .. } => &[WindowKind::Loom],
            Self::Cartography { .. } => &[WindowKind::Cartography],
            Self::Recipe { .. } | Self::Predicted { .. } => &[
                WindowKind::Stonecutter,
                WindowKind::Smithing,
                WindowKind::Cartography,
            ],
        }
    }
}

/// Temporary inputs and the ordinary manager's distinct network projection.
#[derive(Clone, Copy)]
pub(super) struct ScreenInputs {
    pub(super) slots: &'static [u8],
    pub(super) ordinary_inputs: usize,
    pub(super) ordinary_extent: usize,
}

/// Declares each screen's inputs once, including ignored ordinary network cells.
pub(super) const fn input_layout(kind: WindowKind) -> Option<ScreenInputs> {
    let (slots, preview, ignored_tail): (&'static [u8], bool, usize) = match kind {
        WindowKind::Trade { modern: true } => (&protocol::MODERN_TRADE_SLOTS, true, 0),
        WindowKind::Trade { modern: false } => (&protocol::LEGACY_TRADE_SLOTS, true, 0),
        WindowKind::Anvil => (&[ANVIL_TARGET_SLOT, ANVIL_MATERIAL_SLOT], true, 0),
        WindowKind::Grindstone => (&[16, 17], true, 0),
        WindowKind::Loom => (&[9, 10, 11], true, 0),
        WindowKind::Stonecutter => (&[3], true, 0),
        // The template is returnable, but incoming ordinary traffic does not write it.
        WindowKind::Smithing => (&[51, 52, 53], true, 1),
        WindowKind::Cartography => (&[12, 13], true, 0),
        WindowKind::Enchanting => (&[ENCHANT_INPUT_SLOT, ENCHANT_INPUT_SLOT + 1], false, 0),
        WindowKind::Beacon => (&[BEACON_PAYMENT_SLOT], false, 0),
        // Workbench cells use UI traffic; its ordinary manager has no writable cells.
        WindowKind::Workbench => (&[], false, 0),
        _ => return None,
    };
    let ordinary_inputs = slots.len() - ignored_tail;
    Some(ScreenInputs {
        slots,
        ordinary_inputs,
        ordinary_extent: ordinary_inputs + preview as usize,
    })
}

/// The temporary UI input slots owned by one screen, excluding its crafting grid.
pub(super) const fn input_slots(kind: WindowKind) -> &'static [u8] {
    match input_layout(kind) {
        Some(layout) => layout.slots,
        None => &[],
    }
}

/// Screens whose server computes the result from the inputs itself. Vanilla sends them no
/// result list, and a dedicated server crashes on one.
const fn server_derives_result(kind: WindowKind) -> bool {
    matches!(
        kind,
        WindowKind::Anvil | WindowKind::Stonecutter | WindowKind::Cartography
    )
}

/// The order a result take consumes the screen's inputs in: the anvil uses up its
/// second item before the target, and the smithing table its template first.
fn consume_order(kind: WindowKind) -> Vec<u8> {
    let mut slots = input_slots(kind).to_vec();
    match kind {
        WindowKind::Anvil => slots.reverse(),
        WindowKind::Smithing => slots.rotate_right(1),
        _ => {}
    }
    slots
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
    /// Selects an offered enchanting option without predicting its item mutation.
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
        self.begin_screen_output_into(craft, CraftSink::Cursor)
    }

    /// Takes the open screen's result into an empty cursor, or spreads it over the player
    /// inventory like a shift-click; the whole result must fit.
    pub fn begin_screen_output_into(
        &mut self,
        craft: &ScreenCraft,
        sink: CraftSink,
    ) -> Result<i32, InventoryGestureError> {
        let personal_generation = self.gesture_preflight(true)?;
        let kind = self
            .window_kind()
            .filter(|kind| craft.kinds().contains(kind))
            .ok_or(InventoryGestureError::InvalidRequest)?;
        match sink {
            CraftSink::Cursor => {
                self.check_surfaces([Cell::Cursor, Cell::CreatedOutput])?;
                if self.view().get(Cell::Cursor).is_some() {
                    return Err(InventoryGestureError::InvalidRequest);
                }
            }
            CraftSink::Inventory => self.check_surfaces([Cell::CreatedOutput])?,
            CraftSink::Player(_) => return Err(InventoryGestureError::InvalidRequest),
        }
        let request_id = self.peek_request_id()?;
        let predicted_distinct = matches!(
            craft,
            ScreenCraft::Trade { .. }
                | ScreenCraft::Predicted { .. }
                | ScreenCraft::Cartography { .. }
                | ScreenCraft::Loom { .. }
                | ScreenCraft::Anvil {
                    output: Some(_),
                    ..
                }
                | ScreenCraft::Grindstone {
                    output: Some(_),
                    ..
                }
        );
        // `materialized`: the predicted stack carries the result's real item data, so it may merge.
        let (preview, predicted_set, materialized) = match craft {
            ScreenCraft::Predicted { output, .. } | ScreenCraft::Cartography { output, .. } => {
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
                    block_runtime_id: i32::from_ne_bytes(output.block_runtime_id.to_ne_bytes()),
                    extra_data: user_data,
                };
                let held = Held {
                    source_stack_id: Some(stack.stack_network_id),
                    source_origin: None,
                    stack,
                    overlay: None,
                    acknowledged_request_id: None,
                };
                // A stonecutter recipe's output is the whole item; other recipe screens carry
                // their inputs' data.
                let materialized = kind == WindowKind::Stonecutter;
                (held.clone(), Some(held), materialized)
            }
            ScreenCraft::Anvil {
                output: Some(output),
                ..
            }
            | ScreenCraft::Grindstone {
                output: Some(output),
                ..
            }
            | ScreenCraft::Trade { output, .. } => {
                let mut stack = output.clone();
                stack.stack_network_id = request_id;
                stack.nbt_digest = Sha256::digest(&stack.extra_data).into();
                let held = Held {
                    source_stack_id: Some(stack.stack_network_id),
                    source_origin: None,
                    stack,
                    overlay: None,
                    acknowledged_request_id: None,
                };
                (held.clone(), Some(held), true)
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
                    source_stack_id: Some(stack.stack_network_id),
                    source_origin: None,
                    stack,
                    overlay: None,
                    acknowledged_request_id: None,
                };
                (held.clone(), Some(held), false)
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
                (preview, None, true)
            }
        };
        let entry = self
            .negotiated_item_entry(preview.stack.network_id)
            .ok_or(InventoryGestureError::InvalidRequest)?;
        let results = Arc::from([CraftResult {
            identifier: Arc::clone(&entry.identifier),
            aux: i32::try_from(preview.stack.metadata).unwrap_or(0),
            count: preview.stack.count,
            block_runtime_id: u32::from_ne_bytes(preview.stack.block_runtime_id.to_ne_bytes()),
            user_data: Arc::clone(&preview.stack.extra_data),
        }]);
        let (first, filter_strings) = match craft {
            // The index names the first filter string even when the request carries none.
            ScreenCraft::Anvil {
                rename,
                recipe_network_id,
                ..
            } => (
                StackRequestAction::CraftRecipeOptional {
                    recipe_network_id: *recipe_network_id,
                    filtered_string_index: 0,
                },
                rename
                    .iter()
                    .map(|name| name.to_string())
                    .collect::<Vec<String>>(),
            ),
            ScreenCraft::Grindstone {
                recipe_network_id,
                repair_cost,
                ..
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
            ScreenCraft::Cartography {
                recipe_network_id,
                name,
                ..
            } => (
                StackRequestAction::CraftRecipeOptional {
                    recipe_network_id: *recipe_network_id,
                    filtered_string_index: 0,
                },
                vec![name.to_string()],
            ),
            ScreenCraft::Recipe { recipe_network_id }
            | ScreenCraft::Trade {
                recipe_network_id, ..
            }
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
        let mut actions = vec![first];
        if !server_derives_result(kind) {
            actions.push(StackRequestAction::CraftResultsDeprecated { results, crafts: 1 });
        }
        let mut groups = Vec::new();
        for slot in consume_order(kind) {
            let cell = Cell::Craft(slot);
            let Some(held) = self.view().get(cell) else {
                continue;
            };
            let amount = match craft {
                ScreenCraft::Trade { consumed, .. } => {
                    consumed[input_slots(kind)
                        .iter()
                        .position(|input| *input == slot)
                        .expect("trade input")]
                }
                ScreenCraft::Anvil {
                    output: Some(_),
                    material_used,
                    ..
                } if slot == ANVIL_MATERIAL_SLOT => *material_used,
                _ => consume_amount(slot, held.stack.count),
            }
            .min(held.stack.count);
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
        let source_id = preview.stack.stack_network_id;
        let output_slot = request_slot(Cell::CreatedOutput, source_id, None)?;
        if let Some(held) = predicted_set {
            groups.push(DeltaGroup::Set {
                cell: Cell::CreatedOutput,
                held,
            });
        }
        let mut registry_bound_merge = false;
        if sink == CraftSink::Cursor {
            let take = u8::try_from(preview.stack.count)
                .map_err(|_| InventoryGestureError::InvalidRequest)?;
            actions.push(StackRequestAction::Take {
                amount: take,
                source: output_slot,
                destination: request_slot(Cell::Cursor, 0, None)?,
            });
            groups.push(DeltaGroup::Transfer {
                source: Cell::CreatedOutput,
                destination: Cell::Cursor,
                amount: preview.stack.count,
                source_id,
                destination_id: None,
                capacity: None,
            });
        } else {
            let capacity = entry_capacity(entry).ok_or(InventoryGestureError::InvalidRequest)?;
            let plan = self
                .spread_plan(&preview.stack, capacity, !materialized)
                .ok_or(InventoryGestureError::InvalidRequest)?;
            for (cell, amount, into) in plan {
                registry_bound_merge |= into.is_some();
                actions.push(StackRequestAction::Place {
                    amount: u8::try_from(amount)
                        .map_err(|_| InventoryGestureError::InvalidRequest)?,
                    source: output_slot,
                    destination: request_slot(cell, into.map_or(0, |(id, _)| id), None)?,
                });
                groups.push(DeltaGroup::Transfer {
                    source: Cell::CreatedOutput,
                    destination: cell,
                    amount,
                    source_id,
                    destination_id: into.map(|(id, _)| id),
                    capacity: into.map(|_| u16::from(capacity)),
                });
            }
        }
        self.submit_filtered(
            Submission {
                actions,
                groups,
                personal_generation,
                requires_distinct_stack_ids: predicted_distinct,
                registry_bound_merge,
            },
            filter_strings,
        )
    }
}
