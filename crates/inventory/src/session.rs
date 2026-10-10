use crate::{
    InventoryEquipmentRouter, PlayerInventoryLedger, SequencedInventoryEvent,
    SequencedLocalEquipment, crafting_authority::CraftingAuthority,
};
use protocol::{InventoryAuthority, InventoryEvent};
use std::{collections::VecDeque, sync::Arc};

/// Maximum queued authoritative inventory events before the caller must apply backpressure.
pub const MAX_PENDING_INVENTORY_EVENTS: usize = 1_024;

/// One session's inventory authority, prediction journal and ordered ingress.
#[derive(Clone, Debug)]
pub struct InventorySession {
    pub(crate) session_id: u64,
    pub(crate) inventory_authority: Option<InventoryAuthority>,
    pub(crate) last_inventory_sequence: Option<u64>,
    pub(crate) pending_inventory: VecDeque<SequencedInventoryEvent>,
    pub(crate) crafting_authority: CraftingAuthority,
    pub(crate) furnace_cache: std::sync::Arc<std::sync::Mutex<crate::furnace_recipes::Cache>>,
    pub(crate) equipment_router: Arc<InventoryEquipmentRouter>,
    pub(crate) local_selected_equipment: Option<SequencedLocalEquipment>,
    pub(crate) local_selected_slot: Option<u8>,
    pub(crate) pending_hotbar_selection: Option<u8>,
    pub(crate) server_selected_slot: Option<u8>,
    pub(crate) inventory_ledger: Arc<PlayerInventoryLedger>,
}

impl InventorySession {
    /// Start with no server authority or retained inventory values.
    pub fn new(session_id: u64) -> Self {
        let mut inventory_ledger = PlayerInventoryLedger::default();
        inventory_ledger.begin_session(session_id);
        Self {
            session_id,
            inventory_authority: None,
            last_inventory_sequence: None,
            pending_inventory: VecDeque::with_capacity(MAX_PENDING_INVENTORY_EVENTS),
            crafting_authority: CraftingAuthority::new(session_id),
            furnace_cache: Default::default(),
            equipment_router: Arc::new(InventoryEquipmentRouter::new(session_id)),
            local_selected_equipment: None,
            local_selected_slot: None,
            pending_hotbar_selection: None,
            server_selected_slot: None,
            inventory_ledger: Arc::new(inventory_ledger),
        }
    }

    /// Retire all inventory state when the connection changes sessions.
    pub fn begin_session(&mut self, session_id: u64) {
        if self.session_id != session_id {
            *self = Self::new(session_id);
        }
    }

    /// Returns a copy of this session that shows `ledger` in place of its own.
    #[must_use]
    pub fn with_ledger(&self, ledger: Arc<PlayerInventoryLedger>) -> Self {
        Self {
            session_id: self.session_id,
            inventory_authority: self.inventory_authority,
            last_inventory_sequence: self.last_inventory_sequence,
            pending_inventory: VecDeque::new(),
            crafting_authority: self.crafting_authority.clone(),
            furnace_cache: std::sync::Arc::clone(&self.furnace_cache),
            equipment_router: Arc::clone(&self.equipment_router),
            local_selected_equipment: self.local_selected_equipment.clone(),
            local_selected_slot: self.local_selected_slot,
            pending_hotbar_selection: self.pending_hotbar_selection,
            server_selected_slot: self.server_selected_slot,
            inventory_ledger: ledger,
        }
    }

    /// The connection generation that owns every retained value.
    pub const fn session_id(&self) -> u64 {
        self.session_id
    }

    /// The negotiated inventory authority, if startup has published it.
    pub const fn inventory_authority(&self) -> Option<InventoryAuthority> {
        self.inventory_authority
    }

    /// Publish negotiated authority through the same ledger transition used by ingress.
    pub fn publish_inventory_authority(&mut self, authority: InventoryAuthority) {
        self.inventory_authority = Some(authority);
        self.ledger_mut()
            .apply(&InventoryEvent::Authority(authority));
    }

    /// Borrow the authoritative and predicted inventory projection.
    pub fn ledger(&self) -> &PlayerInventoryLedger {
        &self.inventory_ledger
    }

    /// Captures immutable presentation cells and metadata without cloning the journal.
    pub fn ledger_snapshot(&self) -> Arc<PlayerInventoryLedger> {
        Arc::clone(&self.inventory_ledger)
    }

    /// Issue synchronous inventory commands through the single ledger owner.
    pub fn ledger_mut(&mut self) -> &mut PlayerInventoryLedger {
        Arc::make_mut(&mut self.inventory_ledger)
    }

    /// Expire outstanding requests; the caller updates its screen if a lifecycle expired.
    pub fn poll_inventory_timeout(&mut self, now_millis: u64) -> bool {
        self.ledger_mut().poll_timeout(now_millis)
    }

    /// Retire requests when their transport closes.
    pub fn inventory_transport_closed(&mut self) {
        self.ledger_mut().transport_closed();
    }

    /// Apply bootstrap registry and negotiation without inventing a FIFO receipt.
    pub fn publish_bootstrap_inventory(
        &mut self,
        registry: Option<protocol::ItemRegistryEvent>,
        event: InventoryEvent,
    ) -> bool {
        let InventoryEvent::Authority(authority) = event else {
            return false;
        };
        self.publish_crafting_bootstrap(registry.as_ref(), authority);
        if let Some(registry) = registry {
            self.ledger_mut().apply_registry(&registry);
        }
        self.publish_inventory_authority(authority);
        true
    }

    /// Number of authoritative events waiting for the frame's ordered drain.
    pub fn pending_inventory_len(&self) -> usize {
        self.pending_inventory.len()
    }
}
