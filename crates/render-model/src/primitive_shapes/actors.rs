//! One compact position entry per attached actor, shared by all of its shapes.

use super::PrimitiveSlots;
use std::collections::HashMap;

/// Attachment deliberately carries translation only, matching vanilla debug drawing.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct PrimitiveActor {
    pub position: [f32; 3],
    pub valid: u32,
}

#[derive(Debug)]
struct ActorReference {
    slot: u32,
    users: usize,
    active_index: usize,
}

#[derive(Debug, Default)]
pub(super) struct Attachments {
    references: HashMap<i64, ActorReference>,
    active: Vec<i64>,
}

impl Attachments {
    /// Reuses an actor entry across all shapes attached to the same unique id.
    pub fn acquire(&mut self, id: i64, slots: &mut PrimitiveSlots<PrimitiveActor>) -> u32 {
        if let Some(reference) = self.references.get_mut(&id) {
            reference.users += 1;
            return reference.slot;
        }
        let slot = slots.insert(PrimitiveActor::default());
        self.references.insert(
            id,
            ActorReference {
                slot,
                users: 1,
                active_index: self.active.len(),
            },
        );
        self.active.push(id);
        slot
    }

    /// Removes unused actors from the compact frame list in constant time.
    pub fn release(&mut self, id: i64, slots: &mut PrimitiveSlots<PrimitiveActor>) {
        let Some(reference) = self.references.get_mut(&id) else {
            return;
        };
        reference.users -= 1;
        if reference.users != 0 {
            return;
        }
        let reference = self.references.remove(&id).expect("reference was present");
        self.active.swap_remove(reference.active_index);
        if let Some(&moved) = self.active.get(reference.active_index) {
            self.references
                .get_mut(&moved)
                .expect("active actor has a slot")
                .active_index = reference.active_index;
        }
        slots.release(reference.slot);
    }

    /// Visits attached actors only; stationary positions produce no uploads.
    pub fn update(
        &self,
        slots: &mut PrimitiveSlots<PrimitiveActor>,
        mut position: impl FnMut(i64) -> Option<[f32; 3]>,
    ) {
        for &id in &self.active {
            let sample = position(id).filter(|p| p.iter().all(|v| v.is_finite()));
            slots.set(
                self.references[&id].slot,
                PrimitiveActor {
                    position: sample.unwrap_or_default(),
                    valid: u32::from(sample.is_some()),
                },
            );
        }
    }
}
