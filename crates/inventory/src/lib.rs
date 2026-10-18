//! Engine-independent inventory authority, prediction, crafting and commands.
mod anvil;
mod crafting;
mod crafting_authority;
pub mod enchantments;
mod inventory_options;
pub use inventory_options::{InventoryContext, InventoryController};
mod furnace_options;
mod furnace_recipes;
mod ingress;
pub mod inventory_ledger;
pub mod inventory_router;
mod item_permissions;
mod manual_craft;
mod matching;
mod screen_recipes;
mod selection;
mod session;
mod session_items;
pub use session_items::{SessionItemFacts, SessionItemKind};
mod transport;

pub use anvil::{ANVIL_TOO_EXPENSIVE, AnvilItem, AnvilOutcome, evaluate_anvil};
pub use crafting::{
    CraftGridItem, CraftGridMatch, ingredient_accepts, match_crafting_grid,
    recipe_ingredient_accepts, screen_ingredient_accepts,
};
pub use crafting_authority::CraftingPreview;
pub use furnace_recipes::{FurnaceRecipeIter, FurnaceRecipes};
pub use ingress::{InventoryAuthorityEvent, InventoryIngressError, SequencedInventoryEvent};
pub use inventory_ledger::{PlayerInventoryLedger, PlayerInventorySlot};
pub use inventory_router::{
    EquipmentRoute, EquipmentRouteResult, InventoryEquipmentRouter, InventoryRouterError,
};
pub use manual_craft::{
    ManualCraftError, ManualCraftInput, ManualCraftSnapshot, manual_craft_packet,
};
pub use matching::{ManualCraftCell, ManualCraftMatch, ManualCraftPreview, match_manual_grid};
pub use screen_recipes::BookRecipe;
pub use selection::{
    PreparedHotbarSelection, SelectedStackSnapshot, SelectionAuthority, SelectionStamp,
    SequencedLocalEquipment,
};
pub use session::{InventorySession, MAX_PENDING_INVENTORY_EVENTS};

mod item_icon;
pub use item_icon::{crossbow_animation_frame, ranged_animation_frame};

mod dial;
pub use dial::{
    CLOCK_FRAMES, COMPASS_FRAMES, ClockReading, CompassReading, DialFrames, DialNeedle,
    natural_dimension,
};
