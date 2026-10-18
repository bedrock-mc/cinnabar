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
    pub(crate) inventory_options: protocol::InventoryOptions,
    pub(crate) furnace_preferences: crate::furnace_options::FurnacePreferences,
    pub(crate) furnace_cache: std::sync::Arc<std::sync::Mutex<crate::furnace_recipes::Cache>>,
    pub(crate) equipment_router: Arc<InventoryEquipmentRouter>,
    pub(crate) local_selected_equipment: Option<SequencedLocalEquipment>,
    pub(crate) local_selected_slot: Option<u8>,
    pub(crate) selection_generation: u64,
    pub(crate) pending_hotbar_selection: Option<u8>,
    pub(crate) announced_offhand: protocol::NetworkItemStack,
    pub(crate) announced_hotbar: (u8, protocol::NetworkItemStack),
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
            inventory_options: Default::default(),
            furnace_preferences: Default::default(),
            furnace_cache: Default::default(),
            equipment_router: Arc::new(InventoryEquipmentRouter::new(session_id)),
            local_selected_equipment: None,
            local_selected_slot: None,
            selection_generation: 0,
            pending_hotbar_selection: None,
            announced_offhand: protocol::NetworkItemStack::empty(),
            announced_hotbar: (0, protocol::NetworkItemStack::empty()),
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
            inventory_options: self.inventory_options,
            furnace_preferences: self.furnace_preferences.clone(),
            furnace_cache: std::sync::Arc::clone(&self.furnace_cache),
            equipment_router: Arc::clone(&self.equipment_router),
            local_selected_equipment: self.local_selected_equipment.clone(),
            local_selected_slot: self.local_selected_slot,
            selection_generation: self.selection_generation,
            pending_hotbar_selection: self.pending_hotbar_selection,
            announced_offhand: self.announced_offhand.clone(),
            announced_hotbar: self.announced_hotbar.clone(),
            server_selected_slot: self.server_selected_slot,
            inventory_ledger: ledger,
        }
    }

    /// Explicit recipe-name authority at the committed inventory frontier.
    pub fn recipe_unlocks(&self) -> &protocol::RecipeUnlockState {
        self.crafting_authority.recipe_unlocks()
    }

    /// Complete recipe-discovery rules at the committed inventory frontier.
    pub fn crafting_rules(&self) -> Option<protocol::CraftingRules> {
        self.crafting_authority.crafting_rules()
    }

    /// Changes whenever committed crafting policy or its input authority changes.
    pub fn crafting_revision(&self) -> u64 {
        self.crafting_authority.revision()
    }

    /// Publishes the StartGame rule defaults before deferred play observations.
    pub fn publish_crafting_rules_bootstrap(&mut self, rules: protocol::CraftingRules) {
        self.crafting_authority.bootstrap_rules(rules);
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

    /// Borrows facts admitted with this session's current item registry.
    pub fn item_facts(&self) -> &crate::SessionItemFacts {
        self.ledger().item_facts()
    }

    /// Captures immutable presentation cells and metadata without cloning the journal.
    pub fn ledger_snapshot(&self) -> Arc<PlayerInventoryLedger> {
        Arc::clone(&self.inventory_ledger)
    }

    /// Issue synchronous inventory commands through the single ledger owner.
    pub fn ledger_mut(&mut self) -> &mut PlayerInventoryLedger {
        let catalog = self.crafting_authority.catalog_snapshot();
        let ledger = Arc::make_mut(&mut self.inventory_ledger);
        ledger.bind_furnace_inputs(catalog);
        ledger
    }

    /// Whether furnace admission already matches the committed recipes, so no binding is due.
    pub(crate) fn furnace_admission_bound(&self) -> bool {
        self.inventory_ledger
            .furnace_inputs_bound(&self.crafting_authority.catalog_snapshot())
    }

    /// Expire outstanding requests; the caller updates its screen if a lifecycle expired.
    pub fn poll_inventory_timeout(&mut self, now_millis: u64) -> bool {
        if !self.ledger().timeout_work_due(now_millis) {
            return false;
        }
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
