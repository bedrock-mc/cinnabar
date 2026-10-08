//! Intents accepted by the Marketplace state machine.

/// A player intent on the store screens, small enough to travel inside a menu action.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoreAction {
    /// The start screen's Marketplace button.
    Open,
    Back,
    Home,
    OpenSearch,
    /// The coin wallet header button.
    CoinWallet,
    /// The player's owned content.
    Inventory,
    /// An offer card; `row` indexes the displayed rows, `index` the offer within it.
    OpenOffer {
        row: u16,
        index: u16,
    },
    /// A row's "show more" button.
    ShowMore {
        row: u16,
    },
    /// The purchase button on the offer page.
    Buy,
    /// A modal's left or single button.
    ModalPrimary,
    /// A modal's right (cancel) button.
    ModalSecondary,
    /// Escape or a click outside a modal.
    ModalDismiss,
}
