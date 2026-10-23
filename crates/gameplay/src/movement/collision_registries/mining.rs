//! Mining components bound to the session's runtime block identities.

impl super::PhysicsCollisionRegistries {
    /// Borrows values decoded during registry preparation; incomplete states stay unknown.
    pub fn block_states(
        &self,
        mode: assets::NetworkIdMode,
        runtime_id: u32,
    ) -> Option<&[(std::sync::Arc<str>, protocol::CustomStateValue)]> {
        let entries = match mode {
            assets::NetworkIdMode::Sequential => &self.interaction_blocks,
            assets::NetworkIdMode::Hashed => &self.hashed_interaction_blocks,
        };
        entries.get(&runtime_id)?.states.as_deref()
    }

    /// Custom definitions carry their whole tag set; native carriers currently carry a subset.
    pub fn block_tags_complete(&self, mode: assets::NetworkIdMode, runtime_id: u32) -> bool {
        let entries = match mode {
            assets::NetworkIdMode::Sequential => &self.interaction_blocks,
            assets::NetworkIdMode::Hashed => &self.hashed_interaction_blocks,
        };
        entries
            .get(&runtime_id)
            .is_some_and(|block| block.custom_state.is_some())
    }
    /// Borrows the resolved mining component and tool mask independently of session artwork.
    pub fn custom_block_mining(
        &self,
        mode: assets::NetworkIdMode,
        runtime_id: u32,
    ) -> Option<(&protocol::CustomBlockMining, u8)> {
        let entries = match mode {
            assets::NetworkIdMode::Sequential => &self.interaction_blocks,
            assets::NetworkIdMode::Hashed => &self.hashed_interaction_blocks,
        };
        let (definition, state) = entries.get(&runtime_id)?.custom_state.as_ref()?;
        let mining = definition.mining_for_state(*state)?;
        let tools = assets::facts::block::custom_block_effective_tools(
            definition.tags.iter().map(AsRef::as_ref),
        );
        Some((mining, tools))
    }
}

/// Decodes native typed states once while preparing the collision registry.
pub(super) fn native_states(
    states: Option<&serde_json::Map<String, serde_json::Value>>,
) -> Option<protocol::DiggerStates> {
    states?
        .iter()
        .map(|(name, typed)| {
            let value = typed.get("value").unwrap_or(typed);
            let value = if let Some(value) = value.as_str() {
                protocol::CustomStateValue::String(value.into())
            } else if let Some(value) = value.as_bool() {
                protocol::CustomStateValue::Bool(value)
            } else {
                protocol::CustomStateValue::Int(value.as_i64()?)
            };
            Some((name.as_str().into(), value))
        })
        .collect()
}

/// Pairs a custom palette state's admitted values with its shared axis names.
pub(super) fn custom_states(
    block: &protocol::CustomBlock,
    state: Option<&protocol::CustomHashedState>,
) -> Option<protocol::DiggerStates> {
    let count = block
        .visual
        .state_axes
        .iter()
        .try_fold(1_u64, |count, axis| {
            count.checked_mul(axis.values.len() as u64)
        });
    if block.visual.state_identity_incomplete
        || block.visual.materialization_disabled
        || count != Some(u64::from(block.state_count))
    {
        return Default::default();
    }
    Some(
        block
            .visual
            .state_axes
            .iter()
            .zip(state?.values.iter())
            .map(|(axis, value)| (std::sync::Arc::clone(&axis.name), value.clone()))
            .collect(),
    )
}
