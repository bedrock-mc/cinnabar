//! Requests and replies exchanged with the host's store workers.

use bridge::{
    BridgeError, ConfirmedPurchase, PurchaseOutcome, StoreBalance, StoreEntitlements,
    StoreOfferDetail, StorePage, StoreRowMore, StoreSearch, StoreSearchResults,
};
use std::path::PathBuf;

/// What went wrong, reduced to what the screens react to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreError {
    /// The account is not signed in (or was signed out).
    SignedOut,
    /// A purchase of the same offer is already running or unresolved.
    Busy,
    /// The core refused the request as malformed or unknown.
    Rejected,
    /// The core or the store service could not be reached.
    Unavailable,
}

impl From<&BridgeError> for StoreError {
    fn from(error: &BridgeError) -> Self {
        match error {
            BridgeError::ControlRpc { code: -32020, .. } => Self::SignedOut,
            BridgeError::ControlRpc { code: -32031, .. } => Self::Busy,
            BridgeError::ControlRpc {
                code: -32602 | -32032 | -32033,
                ..
            } => Self::Rejected,
            _ => Self::Unavailable,
        }
    }
}

#[derive(Clone, Debug)]
pub enum StoreRequest {
    /// A known page name, or `None` for the store home.
    Home(Option<String>),
    Search(StoreSearch),
    Offer(String),
    Balance,
    Entitlements {
        offset: u32,
        /// Ask the service to refresh the inventory first (the first window only).
        refresh: bool,
    },
    /// More offers for page row `row`, from its continuation token.
    RowMore {
        row: usize,
        continuation: String,
    },
    Purchase(ConfirmedPurchase),
    Image(String),
}

#[derive(Debug)]
pub enum StoreEvent {
    Page(Result<StorePage, StoreError>),
    Search(Result<StoreSearchResults, StoreError>),
    Offer(Result<Box<StoreOfferDetail>, StoreError>),
    OfferFailed {
        id: String,
        error: StoreError,
    },
    Balance(Result<Vec<StoreBalance>, StoreError>),
    Entitlements {
        offset: u32,
        result: Result<StoreEntitlements, StoreError>,
    },
    RowMore {
        row: usize,
        result: Result<StoreRowMore, StoreError>,
    },
    Purchase {
        purchase_id: String,
        result: Result<PurchaseOutcome, StoreError>,
    },
    Image {
        url: String,
        result: Result<PathBuf, StoreError>,
    },
}
