//! Store screen bindings and artwork over launcher-owned state.
mod action;
mod bindings;
mod flow;
pub mod images;
mod screens;
use json_ui::HitRegion;
pub use launcher::store::{SDL_SCREEN, StoreAction, StoreSnapshot, StoreState};
pub use screens::{ScreenSpec, StoreScreens, screens};
/// Resolves a pressed store region against the current launcher snapshot.
pub fn action(snapshot: Option<&StoreSnapshot>, region: &HitRegion) -> Option<StoreAction> {
    let snapshot = snapshot?;
    action::action_for(snapshot.view, snapshot.modal_active(), region)
}
/// The start-screen Marketplace button.
pub const OPEN: StoreAction = StoreAction::Open;

pub use launcher::store::{snapshot, state, worker};
