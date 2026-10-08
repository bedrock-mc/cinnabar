//! Immutable session item presentation inputs.
pub use assets::SessionEntityPack;
use std::sync::Arc;
/// StartGame item components and pack item icons the equipment layer draws custom items from.
#[derive(Debug)]
pub struct SessionItems {
    pub components: Arc<client_ui::ui_runtime::item_facts::SessionItemComponents>,
    pub icons: Option<Arc<client_ui::ui_runtime::presentation::SessionIcons>>,
}
