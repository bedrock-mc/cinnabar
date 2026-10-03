//! Opt-in local observation of normalized inbound stone identity, not use success.

use protocol::{
    CanonicalCell, InventoryEvent, ItemRegistryEntry, ItemRegistryVersion, NetworkItemStack,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::{InventoryAuthorityEvent, UiRuntime, inventory_ledger::PlayerInventorySlot};

const MAX_ROWS: usize = 8;
const TARGET: &str = "minecraft:stone";

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct StackIdentity {
    item_network_id: i32,
    block_runtime_bits: u32,
    count: u16,
    metadata: u32,
    stack_network_id: i32,
    normalized_extra_sha256: [u8; 32],
    normalized_extra_byte_length: usize,
}

impl StackIdentity {
    fn from_stack(stack: &NetworkItemStack) -> Option<Self> {
        if stack.is_empty() || stack.stack_network_id <= 0 {
            return None;
        }
        let digest: [u8; 32] = Sha256::digest(&stack.extra_data).into();
        if digest != stack.nbt_digest {
            return None;
        }
        Some(Self {
            item_network_id: stack.network_id,
            block_runtime_bits: u32::from_ne_bytes(stack.block_runtime_id.to_ne_bytes()),
            count: stack.count,
            metadata: stack.metadata,
            stack_network_id: stack.stack_network_id,
            normalized_extra_sha256: digest,
            normalized_extra_byte_length: stack.extra_data.len(),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct Identity {
    /// Current client-resolved selection; this is not a server selection confirmation.
    client_selected_slot: u8,
    item_version_wire_value: i32,
    component_based: bool,
    canonical_empty_component_data: bool,
    normalized_component_sha256: [u8; 32],
    stack: StackIdentity,
}

#[derive(Clone, Debug, Serialize)]
struct Row {
    schema: &'static str,
    identifier: &'static str,
    session_generation: u64,
    observed_fifo_sequence: u64,
    registry_observed_fifo_sequence: u64,
    stack_observed_fifo_sequence: u64,
    identity: Identity,
}

// Cloning a UI snapshot must preserve the observation quota, FIFO and dedup state.
#[derive(Clone, Debug)]
pub(super) struct UseOnIdentityEvidence {
    enabled: bool,
    session: u64,
    last_sequence: Option<u64>,
    registry: Option<(u64, ItemRegistryEntry)>,
    stack_sources: [Option<(u64, StackIdentity)>; 9],
    rows: Vec<Row>,
}

impl UseOnIdentityEvidence {
    pub(super) fn from_environment(session: u64) -> Self {
        // Explicitly select the one ordinary acceptance target; other values are off.
        Self::new(
            std::env::var(crate::acceptance::markers::USE_ON_IDENTITY_EVIDENCE)
                .is_ok_and(|value| value == "stone"),
            session,
        )
    }

    fn new(enabled: bool, session: u64) -> Self {
        Self {
            enabled,
            session,
            last_sequence: None,
            registry: None,
            stack_sources: std::array::from_fn(|_| None),
            rows: Vec::new(),
        }
    }

    pub(super) fn reset(&mut self, session: u64) {
        *self = Self::new(self.enabled, session);
    }

    fn admit_sequence(&mut self, session: u64, sequence: u64) -> bool {
        if !self.enabled
            || self.rows.len() >= MAX_ROWS
            || self.session != session
            || self
                .last_sequence
                .is_some_and(|previous| sequence <= previous)
        {
            return false;
        }
        self.last_sequence = Some(sequence);
        true
    }

    fn note_stack(
        &mut self,
        sequence: u64,
        container: &protocol::ContainerIdentity,
        slot: u16,
        stack: &NetworkItemStack,
    ) {
        if let Some(CanonicalCell::PlayerInventory(slot)) =
            protocol::project_container_cell(container, slot)
            && let Some(source) = self.stack_sources.get_mut(usize::from(slot))
        {
            *source = StackIdentity::from_stack(stack).map(|identity| (sequence, identity));
        }
    }

    fn note_inventory(&mut self, sequence: u64, event: &InventoryEvent) {
        for update in event.slot_updates() {
            self.note_stack(
                sequence,
                &update.identity.container,
                update.identity.slot,
                &update.stack,
            );
        }
        if let InventoryEvent::Content(event) = event {
            for (slot, stack) in event.slots.iter().take(9).enumerate() {
                self.note_stack(sequence, &event.container, slot as u16, stack);
            }
        }
    }
}

impl UiRuntime {
    pub(super) fn observe_use_on_identity(
        &mut self,
        player_runtime: &crate::player_runtime::PlayerRuntime,
        session: u64,
        sequence: u64,
        event: &InventoryAuthorityEvent,
    ) {
        if session != self.session_id
            || !self
                .use_on_identity_evidence
                .admit_sequence(session, sequence)
        {
            return;
        }
        match event {
            InventoryAuthorityEvent::Registry(event) => {
                self.use_on_identity_evidence.registry = event
                    .entries
                    .iter()
                    .find(|entry| entry.identifier.as_ref() == TARGET)
                    .filter(|entry| {
                        player_runtime
                            .inventory
                            .ledger()
                            .negotiated_item_entry(entry.network_id)
                            == Some(*entry)
                    })
                    .map(|entry| (sequence, entry.clone()));
            }
            InventoryAuthorityEvent::Inventory(event) => self
                .use_on_identity_evidence
                .note_inventory(sequence, event),
        }
        let Some(slot) = self.selected_hotbar_slot(player_runtime) else {
            return;
        };
        if player_runtime
            .inventory
            .pending_hotbar_selection()
            .is_some()
            || player_runtime
                .inventory
                .ledger()
                .pending_request_id()
                .is_some()
            || player_runtime.inventory.ledger().resync_required()
        {
            return;
        }
        // Selection is client intent, not server confirmation or transport success.
        // Never substitute equipment/bootstrap or predicted stacks for ledger authority.
        let Some(PlayerInventorySlot::Present(stack)) =
            player_runtime.inventory.ledger().slot_state(slot)
        else {
            return;
        };
        let Some((registry_sequence, entry)) = &self.use_on_identity_evidence.registry else {
            return;
        };
        if entry.network_id != stack.network_id
            || player_runtime
                .inventory
                .ledger()
                .negotiated_item_entry(stack.network_id)
                != Some(entry)
        {
            return;
        }
        let Some((stack_sequence, stack_identity)) = self
            .use_on_identity_evidence
            .stack_sources
            .get(usize::from(slot))
            .and_then(Option::as_ref)
        else {
            return;
        };
        let Some(current_stack) = StackIdentity::from_stack(stack) else {
            return;
        };
        if current_stack != *stack_identity {
            return;
        }
        let version = match entry.version {
            ItemRegistryVersion::Legacy => 0,
            ItemRegistryVersion::DataDriven => 1,
            ItemRegistryVersion::None => 2,
            ItemRegistryVersion::Unknown(value) => value,
        };
        let identity = Identity {
            client_selected_slot: slot,
            item_version_wire_value: version,
            component_based: entry.component_based,
            canonical_empty_component_data: entry.canonical_empty_component_data,
            normalized_component_sha256: entry.component_digest,
            stack: current_stack,
        };
        if self
            .use_on_identity_evidence
            .rows
            .iter()
            .any(|row| row.identity == identity)
        {
            return;
        }
        let row = Row {
            schema: "cinnabar-use-on-inbound-identity-v1",
            identifier: TARGET,
            session_generation: session,
            observed_fifo_sequence: sequence,
            registry_observed_fifo_sequence: *registry_sequence,
            stack_observed_fifo_sequence: *stack_sequence,
            identity,
        };
        if let Ok(encoded) = serde_json::to_string(&row) {
            bevy::log::info!(target: "bedrock_client::use_on_inbound_identity", row = %encoded,
                "normalized inbound identity observation; not outgoing or use confirmation");
        }
        self.use_on_identity_evidence.rows.push(row);
    }
}

#[cfg(test)]
mod tests;
