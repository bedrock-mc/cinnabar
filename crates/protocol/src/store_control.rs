//! Marketplace control client (home, search, offer, balance, entitlements, purchase), re-exported so
//! the app reaches the bridge through this facade.

pub use bridge::{
    BridgeError, ConfirmedPurchase, PendingPurchase, PurchaseOutcome, PurchaseStatus, StoreBalance,
    StoreEntitlements, StoreOffer, StoreOfferDetail, StorePage, StorePrice, StoreRating, StoreRow,
    StoreRowMore, StoreSearch, StoreSearchResults, store_balance, store_entitlements, store_home,
    store_offer, store_purchase, store_row_more, store_search,
};
