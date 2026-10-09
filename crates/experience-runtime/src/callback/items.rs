//! The actor's inventory and the stacks a guest makes: `inventory`, `set-slot` and `drop-item`.

use anyhow::Result;

use super::{CallbackRes, Reach, stage};
use crate::hex;
use crate::host::cinnabar::experience_server::types::{self as wit, WorldError};
use crate::limits::{MAX_ITEM_DATA_BYTES, MAX_STACK_SIZE};
use crate::load::Catalog;
use crate::protocol::{
    BlockPos, HOTBAR_SLOTS, INVENTORY_SLOTS, Inventory, ItemStack, NewStack, Op,
};

/// Checks the actor's inventory as the adapter snapshotted it: [`INVENTORY_SLOTS`] slots, the
/// selected one in the hotbar, each stack within its most, and data only on the Experience's own
/// items, lowercase hex of at most [`MAX_ITEM_DATA_BYTES`]. Anything else is malformed.
pub(super) fn check_inventory(
    own: &Catalog,
    inventory: &Inventory,
    actor: Option<&str>,
) -> Result<(), String> {
    if actor.is_none() {
        return Err("an inventory without an actor".to_owned());
    }
    if inventory.slots.len() != INVENTORY_SLOTS {
        return Err(format!(
            "an inventory of {} slots; it has {INVENTORY_SLOTS}",
            inventory.slots.len()
        ));
    }
    if inventory.selected >= HOTBAR_SLOTS {
        return Err(format!(
            "selected slot {} is outside the hotbar",
            inventory.selected
        ));
    }
    for (slot, stack) in inventory.slots.iter().enumerate() {
        let Some(stack) = stack else { continue };
        if stack.count == 0 || stack.max_count == 0 || stack.count > stack.max_count {
            return Err(format!(
                "slot {slot} holds {} of {}, more than its most, {}, or none",
                stack.count, stack.id, stack.max_count
            ));
        }
        let Some(data) = &stack.data else { continue };
        if !own.items.contains_key(&stack.id) {
            return Err(format!(
                "slot {slot} has data on {}, which is not its own item",
                stack.id
            ));
        }
        let data = hex::decode(data)
            .map_err(|error| format!("slot {slot}: item data is not lowercase hex: {error}"))?;
        if data.len() > MAX_ITEM_DATA_BYTES {
            return Err(format!(
                "slot {slot} has {} bytes of item data; the limit is {MAX_ITEM_DATA_BYTES}",
                data.len()
            ));
        }
    }
    Ok(())
}

impl CallbackRes {
    /// The actor's inventory with the staged slots applied; `player-unavailable` without an actor.
    pub(crate) fn inventory(&mut self) -> Result<Result<Inventory, WorldError>> {
        self.host_call()?;
        Ok(self.inventory.clone().ok_or(WorldError::PlayerUnavailable))
    }

    /// Stages a slot's new content, a stack the guest may make, or none; a slot written again
    /// replaces its staged op.
    pub(crate) fn set_slot(
        &mut self,
        slot: u32,
        stack: Option<NewStack>,
    ) -> Result<Result<(), WorldError>> {
        self.host_call()?;
        let Some(inventory) = &self.inventory else {
            return Ok(Err(WorldError::PlayerUnavailable));
        };
        let index = slot as usize;
        if index >= inventory.slots.len() {
            return Ok(Err(WorldError::OutOfBounds));
        }
        let made = match stack
            .as_ref()
            .map(|stack| made(&self.own, stack))
            .transpose()
        {
            Ok(made) => made,
            Err(error) => return Ok(Err(error)),
        };
        if index == INVENTORY_SLOTS - 1
            && stack.as_ref().is_some_and(|stack| {
                self.own
                    .server
                    .get(&stack.id)
                    .and_then(|variants| variants.get(&stack.metadata))
                    .is_none_or(|variant| !variant.off_hand)
            })
        {
            return Ok(Err(WorldError::UnsupportedState));
        }
        let op = Op::SetSlot { slot, stack };
        let earlier = self
            .ops
            .iter()
            .position(|op| matches!(op, Op::SetSlot { slot: at, .. } if *at == slot));
        match earlier {
            Some(index) => self.ops[index] = op,
            None => stage(&mut self.ops, op)?,
        }
        if let Some(inventory) = &mut self.inventory {
            inventory.slots[index] = made;
        }
        Ok(Ok(()))
    }

    /// Stages an item entity holding a stack the guest may make, spawned at a position
    /// `set-block` may write.
    pub(crate) fn drop_item(
        &mut self,
        pos: BlockPos,
        stack: NewStack,
    ) -> Result<Result<(), WorldError>> {
        self.host_call()?;
        if let Err(error) = self.snapshot.write(pos, Reach::Column) {
            return Ok(Err(error));
        }
        if let Err(error) = made(&self.own, &stack) {
            return Ok(Err(error));
        }
        stage(&mut self.ops, Op::DropItem { pos, stack })?;
        Ok(Ok(()))
    }
}

/// Builds a staged inventory entry using its catalogue rules and adapter classification.
/// Own items may carry data; block and vanilla stacks require no data.
fn made(own: &Catalog, stack: &NewStack) -> Result<ItemStack, WorldError> {
    let data_len = stack.data.as_ref().map_or(0, |data| data.len() / 2);
    if stack.count == 0 {
        return Err(WorldError::UnsupportedState);
    }
    let mut plain = true;
    let max_count = if let Some(&max) = own.items.get(&stack.id) {
        if stack.metadata != 0 {
            return Err(WorldError::UnsupportedState);
        }
        if data_len > MAX_ITEM_DATA_BYTES {
            return Err(WorldError::TooLarge);
        }
        max
    } else {
        let max = if own.blocks.contains_key(&stack.id) {
            if stack.metadata != 0 {
                return Err(WorldError::UnsupportedState);
            }
            MAX_STACK_SIZE
        } else {
            let variant = own
                .server
                .get(&stack.id)
                .ok_or(WorldError::UnknownBlock)?
                .get(&stack.metadata)
                .ok_or(WorldError::UnsupportedState)?;
            plain = variant.plain;
            variant.max_count
        };
        if stack.data.is_some() {
            return Err(WorldError::NotOwned);
        }
        max
    };
    if stack.count > max_count {
        return Err(WorldError::TooLarge);
    }
    Ok(ItemStack {
        id: stack.id.clone(),
        metadata: stack.metadata,
        count: stack.count,
        max_count,
        data: stack.data.clone(),
        plain,
    })
}

impl From<wit::NewStack> for NewStack {
    fn from(stack: wit::NewStack) -> Self {
        Self {
            id: stack.id,
            metadata: stack.metadata,
            count: stack.count,
            data: stack.data.as_deref().map(hex::encode),
        }
    }
}

impl From<Inventory> for wit::Inventory {
    fn from(inventory: Inventory) -> Self {
        let stack = |stack: ItemStack| wit::ItemStack {
            id: stack.id,
            metadata: stack.metadata,
            count: stack.count,
            max_count: stack.max_count,
            // The snapshot's data was checked when the callback was prepared, and staged data is
            // the guest's own bytes encoded.
            data: stack
                .data
                .map(|data| hex::decode(&data).expect("checked item data")),
            plain: stack.plain,
        };
        Self {
            selected: inventory.selected,
            slots: inventory
                .slots
                .into_iter()
                .map(|slot| slot.map(stack))
                .collect(),
        }
    }
}
