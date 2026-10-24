//! Held dials (compasses and clocks) draw their current strip frame.

use super::{EquipmentRuntime, WornItem};

impl EquipmentRuntime {
    /// Sets the strip frames held dials show from now on.
    pub fn set_dial_frames(&mut self, frames: inventory::DialFrames) {
        self.dial_frames = frames;
    }

    /// The vanilla icon sprite a held `item` draws: a dial's current strip frame when its
    /// frames are carried, else the item's own icon.
    pub(super) fn held_icon_index(&self, item: &WornItem) -> Option<usize> {
        let (identifier, metadata) = self
            .dial_frames
            .icon_key(&item.identifier, &self.icons)
            .unwrap_or((&item.identifier, item.metadata));
        self.icons.lookup_held_index(identifier, metadata)
    }
}
