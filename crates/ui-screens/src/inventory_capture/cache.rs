//! Inventory projections retained independently of per-frame HUD publication.

use super::*;
use assets::icon::dial::DialKind;
use inventory::{DialFrames, inventory_ledger::InventoryPresentationSnapshot};

/// Replaced external sources, compared by their owner before capture.
#[derive(Clone, Copy, Default)]
pub struct ProjectionRefresh {
    pub icons: bool,
    pub durability: bool,
    pub names: bool,
    pub recipes: bool,
}

/// Animation inputs that can change without a ledger write.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct ProjectionAnimation {
    pub player: [Option<u32>; protocol::HOTBAR_SLOT_COUNT as usize],
    pub dials: DialFrames,
}

/// Borrowed authority and display state for one capture.
pub struct ProjectionInput<'a> {
    pub ledger: &'a PlayerInventoryLedger,
    pub inventory_open: bool,
    pub refresh: ProjectionRefresh,
    pub animation: ProjectionAnimation,
}

/// Artwork and its clock dependency, resolved together from canonical item facts.
#[derive(Clone, Copy, Default)]
pub struct ProjectedIcon {
    pub icon: Option<IconRef>,
    pub dial: Option<DialKind>,
}

/// Only changed projections need to replace the published HUD values.
#[derive(Default)]
pub struct ProjectionUpdate {
    pub contents: Option<InventoryArtwork>,
    pub item_names: Option<HashMap<(i32, u32), Arc<str>>>,
    pub decorations: Option<(Durability, [Option<IconRef>; protocol::UI_SLOT_COUNT])>,
}

/// Inventory artwork can change without copying the retained hover-name map.
pub struct InventoryArtwork {
    pub inventory_icons: InventoryIcons,
    pub storage_icons: StorageIcons,
    pub crafting: CraftingFrame,
}

#[derive(Default)]
struct CellProjection {
    stack: Option<NetworkItemStack>,
    correction: Option<i32>,
    stack_changed: bool,
    correction_changed: bool,
    artwork: ProjectedIcon,
    local_frame: Option<u32>,
    dial_frame: Option<u32>,
    durability: Option<f32>,
    name: Option<Arc<str>>,
}

impl CellProjection {
    /// Copies a changed cell, preserving memoized projections for equal inputs.
    fn read(&mut self, stack: Option<&NetworkItemStack>, correction: Option<i32>) {
        self.stack_changed = self.stack.as_ref() != stack;
        self.correction_changed = self.correction != correction;
        if self.stack_changed {
            self.stack = stack.cloned();
        }
        self.correction = correction;
    }

    /// Refreshes only artwork affected by a stack, source or animation change.
    fn icon(
        &mut self,
        force: bool,
        local_frame: Option<u32>,
        dials: DialFrames,
        resolve: &mut impl FnMut(&NetworkItemStack, Option<u32>) -> ProjectedIcon,
    ) -> bool {
        let dial_frame = self.artwork.dial.and_then(|kind| dials.frame(kind));
        if !force
            && !self.stack_changed
            && self.local_frame == local_frame
            && self.dial_frame == dial_frame
        {
            return false;
        }
        self.artwork = self
            .stack
            .as_ref()
            .map_or_else(ProjectedIcon::default, |stack| resolve(stack, local_frame));
        self.local_frame = local_frame;
        self.dial_frame = self.artwork.dial.and_then(|kind| dials.frame(kind));
        true
    }

    /// Updates bars independently of animated artwork and translated names.
    fn bar(
        &mut self,
        force: bool,
        fraction: &mut impl FnMut(&NetworkItemStack, Option<i32>) -> Option<f32>,
    ) -> bool {
        if !force && !self.stack_changed && !self.correction_changed {
            return false;
        }
        self.durability = self
            .stack
            .as_ref()
            .and_then(|stack| fraction(stack, self.correction));
        true
    }

    /// Resolves names only while an open inventory consumes them.
    fn name(
        &mut self,
        force: bool,
        resolve: &mut impl FnMut(&NetworkItemStack) -> Option<String>,
    ) -> bool {
        if !force && !self.stack_changed {
            return false;
        }
        self.name = self.stack.as_ref().and_then(resolve).map(Arc::from);
        true
    }

    /// Marks the current source as fully observed for the next capture.
    fn finish(&mut self) {
        self.stack_changed = false;
        self.correction_changed = false;
    }
}

/// Retains source allocations and resolves only invalidated cells or projections.
pub struct InventoryProjectionCache {
    snapshot: Option<InventoryPresentationSnapshot>,
    animation: ProjectionAnimation,
    inventory_open: bool,
    player: [CellProjection; protocol::PLAYER_INVENTORY_SLOTS as usize],
    visual_storage: [CellProjection; protocol::MAX_GENERIC_STORAGE_SLOTS],
    storage: [CellProjection; protocol::MAX_GENERIC_STORAGE_SLOTS],
    ui: [CellProjection; protocol::UI_SLOT_COUNT],
    output: CellProjection,
    contents: InventoryContents,
    durability: Durability,
    ui_icons: [Option<IconRef>; protocol::UI_SLOT_COUNT],
}

impl Default for InventoryProjectionCache {
    fn default() -> Self {
        Self {
            snapshot: None,
            animation: Default::default(),
            inventory_open: false,
            player: std::array::from_fn(|_| CellProjection::default()),
            visual_storage: std::array::from_fn(|_| CellProjection::default()),
            storage: std::array::from_fn(|_| CellProjection::default()),
            ui: std::array::from_fn(|_| CellProjection::default()),
            output: CellProjection::default(),
            contents: Default::default(),
            durability: Default::default(),
            ui_icons: [None; protocol::UI_SLOT_COUNT],
        }
    }
}

impl InventoryProjectionCache {
    /// Captures the same display as the uncached projections without idle cell probes.
    pub fn capture(
        &mut self,
        input: ProjectionInput<'_>,
        crafting_match: impl FnOnce() -> inventory::CraftGridMatch,
        mut resolve_icon: impl FnMut(&NetworkItemStack, Option<u32>) -> ProjectedIcon,
        mut fraction: impl FnMut(&NetworkItemStack, Option<i32>) -> Option<f32>,
        mut item_name: impl FnMut(&NetworkItemStack) -> Option<String>,
    ) -> ProjectionUpdate {
        let ProjectionInput {
            ledger,
            inventory_open,
            refresh,
            animation,
        } = input;
        let first = self.snapshot.is_none();
        let cells_changed = self
            .snapshot
            .as_ref()
            .is_none_or(|old| !old.matches(ledger));
        let open_changed = self.inventory_open != inventory_open;
        if !cells_changed
            && !open_changed
            && self.animation == animation
            && !refresh.icons
            && !refresh.durability
            && !refresh.names
            && !refresh.recipes
        {
            return ProjectionUpdate::default();
        }
        if cells_changed {
            self.read_cells(ledger);
            self.snapshot = Some(ledger.presentation_snapshot());
        }
        let icons = first || refresh.icons;
        let bars = first || refresh.durability;
        let names = first || refresh.names || open_changed;
        let mut contents_changed = first || open_changed;
        let mut decorations_changed = first;
        let mut names_changed = first || open_changed;
        for (slot, cell) in self.player.iter_mut().enumerate() {
            contents_changed |= cell.icon(
                icons,
                animation.player.get(slot).copied().flatten(),
                animation.dials,
                &mut resolve_icon,
            );
            decorations_changed |= cell.bar(bars, &mut fraction);
            if inventory_open {
                names_changed |= cell.name(names, &mut item_name);
            }
        }
        for cell in &mut self.visual_storage {
            contents_changed |= cell.icon(icons, None, animation.dials, &mut resolve_icon);
            if inventory_open {
                names_changed |= cell.name(names, &mut item_name);
            }
        }
        for cell in &mut self.storage {
            decorations_changed |= cell.bar(bars, &mut fraction);
        }
        for cell in &mut self.ui {
            decorations_changed |= cell.icon(icons, None, animation.dials, &mut resolve_icon);
            decorations_changed |= cell.bar(bars, &mut fraction);
        }
        if inventory_open && (first || cells_changed || refresh.recipes || open_changed) {
            let stack = match crafting_match() {
                inventory::CraftGridMatch::Unique(recipe) => {
                    let output = recipe.output();
                    Some(NetworkItemStack {
                        network_id: output.network_id,
                        metadata: u32::from(output.aux),
                        count: u16::from(output.count),
                        block_runtime_id: output.block_runtime_id as i32,
                        ..NetworkItemStack::empty()
                    })
                }
                _ => ledger.crafting_ghost_output().cloned(),
            };
            self.output.read(stack.as_ref(), None);
        }
        if inventory_open {
            self.output.icon(
                icons || open_changed,
                None,
                animation.dials,
                &mut resolve_icon,
            );
        }
        let crafting = if inventory_open {
            let mut crafting = CraftingFrame::default();
            for (icon, slot) in crafting
                .icons
                .iter_mut()
                .zip(ledger.crafting_grid().slots())
            {
                *icon = self.ui[usize::from(slot)].artwork.icon;
            }
            crafting.output = self
                .output
                .stack
                .clone()
                .map(|stack| (self.output.artwork.icon, stack));
            crafting
        } else {
            CraftingFrame::default()
        };
        contents_changed |= self.contents.crafting != crafting;
        self.contents.crafting = crafting;
        if names_changed {
            self.contents.item_names = if inventory_open {
                self.player
                    .iter()
                    .chain(&self.visual_storage)
                    .filter_map(|cell| {
                        let stack = cell.stack.as_ref()?;
                        Some(((stack.network_id, stack.metadata), cell.name.clone()?))
                    })
                    .collect()
            } else {
                HashMap::new()
            };
        }
        if contents_changed {
            self.contents.inventory_icons.0 =
                std::array::from_fn(|slot| self.player[slot].artwork.icon);
            self.contents.storage_icons.0 =
                std::array::from_fn(|slot| self.visual_storage[slot].artwork.icon);
        }
        if decorations_changed {
            self.durability.player = std::array::from_fn(|slot| self.player[slot].durability);
            self.durability.storage = std::array::from_fn(|slot| self.storage[slot].durability);
            self.durability.ui = std::array::from_fn(|slot| self.ui[slot].durability);
            self.ui_icons = std::array::from_fn(|slot| self.ui[slot].artwork.icon);
        }
        for cell in self
            .player
            .iter_mut()
            .chain(&mut self.visual_storage)
            .chain(&mut self.storage)
            .chain(&mut self.ui)
            .chain(std::iter::once(&mut self.output))
        {
            cell.finish();
        }
        self.inventory_open = inventory_open;
        self.animation = animation;
        ProjectionUpdate {
            contents: contents_changed.then(|| InventoryArtwork {
                inventory_icons: self.contents.inventory_icons.clone(),
                storage_icons: self.contents.storage_icons.clone(),
                crafting: self.contents.crafting.clone(),
            }),
            item_names: names_changed.then(|| self.contents.item_names.clone()),
            decorations: decorations_changed.then(|| (self.durability.clone(), self.ui_icons)),
        }
    }

    /// Reads distinct furnace visual and ordinary storage sources after a ledger change.
    fn read_cells(&mut self, ledger: &PlayerInventoryLedger) {
        for (slot, cell) in self.player.iter_mut().enumerate() {
            let slot = slot as u8;
            cell.read(
                ledger.displayed_stack(slot),
                ledger
                    .presented_slot_overlay(slot)
                    .and_then(|overlay| overlay.durability_correction),
            );
        }
        for (slot, cell) in self.visual_storage.iter_mut().enumerate() {
            cell.read(ledger.furnace_visual_stack(slot as u8), None);
        }
        for (slot, cell) in self.storage.iter_mut().enumerate() {
            cell.read(ledger.storage_stack(slot as u8), None);
        }
        for (slot, cell) in self.ui.iter_mut().enumerate() {
            let slot = slot as u8;
            let stack = if slot == protocol::CREATED_OUTPUT_SLOT {
                ledger.created_output_stack()
            } else if protocol::ui_slot_container_name(slot).is_some() {
                ledger
                    .target_stack(InventoryTarget::Craft(slot))
                    .or_else(|| ledger.crafting_ghost_stack(slot))
            } else {
                None
            };
            cell.read(stack, None);
        }
    }
}

#[cfg(test)]
mod tests;
