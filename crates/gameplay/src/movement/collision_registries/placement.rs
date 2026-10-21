use super::PhysicsCollisionRegistries;
use assets::NetworkIdMode;

impl PhysicsCollisionRegistries {
    /// Borrows the authored definition and current state of an admitted custom runtime ID.
    pub(crate) fn custom_placement_state(
        &self,
        mode: NetworkIdMode,
        runtime_id: u32,
    ) -> Option<(&protocol::CustomBlock, u32)> {
        let blocks = match mode {
            NetworkIdMode::Sequential => &self.interaction_blocks,
            NetworkIdMode::Hashed => &self.hashed_interaction_blocks,
        };
        let (definition, index) = blocks.get(&runtime_id)?.custom_state.as_ref()?;
        Some((definition, *index))
    }

    /// Resolves a custom palette index only when the selected ID belongs to this admission.
    pub(crate) fn custom_placement_runtime_id(
        &self,
        mode: NetworkIdMode,
        held: u32,
        index: u32,
    ) -> Option<u32> {
        let (definition, held_index) = self.custom_placement_state(mode, held)?;
        if index >= definition.state_count {
            return None;
        }
        let runtime_id = match mode {
            NetworkIdMode::Sequential => held.checked_sub(held_index)?.checked_add(index)?,
            NetworkIdMode::Hashed => definition.hashed_states().get(index as usize)?.hash,
        };
        let (admitted, admitted_index) = self.custom_placement_state(mode, runtime_id)?;
        (std::ptr::eq(definition, admitted) && admitted_index == index).then_some(runtime_id)
    }
}
