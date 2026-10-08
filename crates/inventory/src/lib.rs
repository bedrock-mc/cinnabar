//! Engine-independent inventory authority, prediction, crafting and commands.
mod crafting;
mod crafting_authority;
mod ingress;
pub mod inventory_ledger;
pub mod inventory_router;
mod manual_craft;
mod matching;
mod screen_recipes;
mod selection;
mod session;
mod transport;

pub use crafting::{
    CraftGridItem, CraftGridMatch, ingredient_accepts, match_crafting_grid,
    recipe_ingredient_accepts, screen_ingredient_accepts,
};
pub use crafting_authority::CraftingPreview;
pub use ingress::{InventoryAuthorityEvent, InventoryIngressError, SequencedInventoryEvent};
pub use inventory_ledger::{PlayerInventoryLedger, PlayerInventorySlot};
pub use inventory_router::{
    EquipmentRoute, EquipmentRouteResult, InventoryEquipmentRouter, InventoryRouterError,
};
pub use manual_craft::{
    ManualCraftError, ManualCraftInput, ManualCraftSnapshot, manual_craft_packet,
};
pub use matching::{ManualCraftCell, ManualCraftMatch, ManualCraftPreview, match_manual_grid};
pub use selection::{SelectedStackSnapshot, SequencedLocalEquipment};
pub use session::{InventorySession, MAX_PENDING_INVENTORY_EVENTS};

mod item_icon;
pub use item_icon::crossbow_animation_frame;
