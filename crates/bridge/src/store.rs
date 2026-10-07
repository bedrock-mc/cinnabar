use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::BridgeError;
use crate::account::call;

/// One currency amount an offer costs; `currency` is the service's own identifier.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct StorePrice {
    pub currency: String,
    pub amount: i64,
}

/// An offer's aggregate star rating.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct StoreRating {
    pub average: f64,
    pub count: u32,
}

/// One store listing as drawn in a row, grid or search result.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct StoreOffer {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub creator: Option<String>,
    #[serde(default)]
    pub content_type: Option<String>,
    #[serde(default)]
    pub thumbnail_url: Option<String>,
    #[serde(default)]
    pub store_id: Option<String>,
    #[serde(default)]
    pub prices: Vec<StorePrice>,
    #[serde(default)]
    pub rating: Option<StoreRating>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub owned: bool,
}

/// An offer plus the fields only the detail screen shows.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct StoreOfferDetail {
    #[serde(flatten)]
    pub offer: StoreOffer,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub screenshot_urls: Vec<String>,
    #[serde(default)]
    pub display_version: Option<String>,
    #[serde(default)]
    pub platforms: Vec<String>,
}

/// A titled strip of offers on a store page.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct StoreRow {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub offers: Vec<StoreOffer>,
    /// Pass to [`store_row_more`] for the row's next offers.
    #[serde(default)]
    pub continuation: Option<String>,
}

/// The next slice of a row's offers.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct StoreRowMore {
    #[serde(default)]
    pub offers: Vec<StoreOffer>,
    #[serde(default)]
    pub continuation: Option<String>,
}

/// A store page reduced to rows of offers.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct StorePage {
    pub id: String,
    #[serde(default)]
    pub rows: Vec<StoreRow>,
    #[serde(default)]
    pub inventory_version: Option<String>,
    #[serde(default)]
    pub truncated: bool,
}

/// One page of search results; pass `continuation` back for the next.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct StoreSearchResults {
    #[serde(default)]
    pub offers: Vec<StoreOffer>,
    #[serde(default)]
    pub continuation: Option<String>,
    #[serde(default)]
    pub truncated: bool,
}

/// A store search, or its continuation; empty fields are omitted.
#[derive(Clone, Debug, Default, Serialize)]
pub struct StoreSearch {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub term: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub continuation: String,
}

/// One virtual currency balance.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct StoreBalance {
    pub currency: String,
    pub amount: i64,
}

/// A window of owned content ids; the next window starts at `offset + owned.len()`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct StoreEntitlements {
    #[serde(default)]
    pub owned: Vec<String>,
    pub total: u32,
    pub offset: u32,
    #[serde(default)]
    pub inventory_version: Option<String>,
}

/// The purchase the confirmation dialog shows; only [`PendingPurchase::confirm`] can turn it into
/// something [`store_purchase`] accepts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingPurchase {
    pub offer_id: String,
    pub store_id: Option<String>,
    pub currency: String,
    pub amount: i64,
    pub unit_duration_seconds: Option<u64>,
}

impl PendingPurchase {
    /// The price entry of `offer` the player chose, or `None` for a free or unpriced entry.
    #[must_use]
    pub fn for_offer(offer: &StoreOffer, price: &StorePrice) -> Option<Self> {
        (price.amount > 0).then(|| Self {
            offer_id: offer.id.clone(),
            store_id: offer.store_id.clone(),
            currency: price.currency.clone(),
            amount: price.amount,
            unit_duration_seconds: None,
        })
    }

    /// Records the player's explicit confirmation; `purchase_id` is the idempotency key, so reuse it
    /// only to retry the same attempt.
    #[must_use]
    pub fn confirm(self, purchase_id: String) -> ConfirmedPurchase {
        ConfirmedPurchase {
            purchase_id,
            offer_id: self.offer_id,
            store_id: self.store_id,
            currency: self.currency,
            amount: self.amount.to_string(),
            unit_duration_seconds: self.unit_duration_seconds,
            confirmed: true,
        }
    }
}

/// A purchase the player confirmed; sent by [`store_purchase`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ConfirmedPurchase {
    purchase_id: String,
    offer_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    store_id: Option<String>,
    currency: String,
    amount: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    unit_duration_seconds: Option<u64>,
    confirmed: bool,
}

impl ConfirmedPurchase {
    /// The idempotency key this purchase was confirmed under.
    #[must_use]
    pub fn purchase_id(&self) -> &str {
        &self.purchase_id
    }
}

/// How the store service answered a purchase.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PurchaseStatus {
    Purchased,
    /// HTTP 422: the shown price no longer matches.
    PriceMismatch,
    /// HTTP 412: the inventory or store state moved on.
    PreconditionFailed,
    Failed,
    /// No definitive answer: refresh balance and entitlements before retrying.
    Unknown,
}

/// The recorded outcome of one purchase attempt.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct PurchaseOutcome {
    pub status: PurchaseStatus,
    #[serde(default)]
    pub http_status: u16,
    #[serde(default)]
    pub marketplace_error_code: u32,
    pub correlation_id: String,
    #[serde(default)]
    pub inventory_version: Option<String>,
    #[serde(default)]
    pub replayed: bool,
}

/// An offer image the core cached on local disk (PNG, JPEG, GIF or BMP).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct StoreImage {
    pub path: std::path::PathBuf,
    pub content_type: String,
}

#[derive(Deserialize)]
struct ImageBody {
    image: StoreImage,
}

#[derive(Serialize)]
struct ImageParams<'a> {
    url: &'a str,
}

#[derive(Deserialize)]
struct PageBody {
    page: StorePage,
}

#[derive(Deserialize)]
struct OfferBody {
    offer: StoreOfferDetail,
}

#[derive(Deserialize)]
struct BalanceBody {
    balances: Vec<StoreBalance>,
}

#[derive(Serialize)]
struct HomeParams<'a> {
    page: &'a str,
}

#[derive(Serialize)]
struct OfferParams<'a> {
    offer_id: &'a str,
}

#[derive(Serialize)]
struct EntitlementParams {
    offset: u32,
    limit: u32,
    refresh: bool,
}

#[derive(Serialize)]
struct RowMoreParams<'a> {
    continuation: &'a str,
}

/// Loads a known store page (`None` is the store home) as rows of offers.
pub async fn store_home(socket_dir: &Path, page: Option<&str>) -> Result<StorePage, BridgeError> {
    let body: PageBody = match page {
        Some(page) => call(socket_dir, "store_home.v1", Some(HomeParams { page })).await?,
        None => call::<_, ()>(socket_dir, "store_home.v1", None).await?,
    };
    Ok(body.page)
}

/// Searches the catalog.
pub async fn store_search(
    socket_dir: &Path,
    search: &StoreSearch,
) -> Result<StoreSearchResults, BridgeError> {
    call(socket_dir, "store_search.v1", Some(search)).await
}

/// Loads one offer's detail.
pub async fn store_offer(
    socket_dir: &Path,
    offer_id: &str,
) -> Result<StoreOfferDetail, BridgeError> {
    let body: OfferBody =
        call(socket_dir, "store_offer.v1", Some(OfferParams { offer_id })).await?;
    Ok(body.offer)
}

/// Reads the virtual currency balances (Minecoins among them).
pub async fn store_balance(socket_dir: &Path) -> Result<Vec<StoreBalance>, BridgeError> {
    let body: BalanceBody = call::<_, ()>(socket_dir, "store_balance.v1", None).await?;
    Ok(body.balances)
}

/// Reads a window of owned content ids; `limit` 0 takes the core's maximum and `refresh` re-reads the
/// inventory from the service (only honored for the first window).
pub async fn store_entitlements(
    socket_dir: &Path,
    offset: u32,
    limit: u32,
    refresh: bool,
) -> Result<StoreEntitlements, BridgeError> {
    call(
        socket_dir,
        "store_entitlements.v1",
        Some(EntitlementParams {
            offset,
            limit,
            refresh,
        }),
    )
    .await
}

/// Loads the next offers of a row from its continuation token.
pub async fn store_row_more(
    socket_dir: &Path,
    continuation: &str,
) -> Result<StoreRowMore, BridgeError> {
    call(
        socket_dir,
        "store_row_more.v1",
        Some(RowMoreParams { continuation }),
    )
    .await
}

/// Downloads an offer image (https only) into the core's bounded cache and returns its path.
pub async fn store_image(socket_dir: &Path, url: &str) -> Result<StoreImage, BridgeError> {
    let body: ImageBody = call(socket_dir, "store_image.v1", Some(ImageParams { url })).await?;
    Ok(body.image)
}

/// Buys a confirmed offer with virtual currency; the core sends it to Mojang at most once per purchase id.
pub async fn store_purchase(
    socket_dir: &Path,
    purchase: &ConfirmedPurchase,
) -> Result<PurchaseOutcome, BridgeError> {
    call(socket_dir, "store_purchase.v1", Some(purchase)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::parse_response;

    #[test]
    fn purchase_serializes_only_after_confirmation_with_a_string_amount() {
        let offer = StoreOffer {
            id: "o1".into(),
            title: "T".into(),
            creator: None,
            content_type: None,
            thumbnail_url: None,
            store_id: Some("s1".into()),
            prices: vec![],
            rating: None,
            tags: vec![],
            owned: false,
        };
        let price = StorePrice {
            currency: "mc".into(),
            amount: 320,
        };
        let pending = PendingPurchase::for_offer(&offer, &price).expect("priced");
        let confirmed = pending.confirm("0123456789abcdef".into());
        assert_eq!(
            serde_json::to_value(&confirmed).expect("encode"),
            serde_json::json!({"purchase_id":"0123456789abcdef","offer_id":"o1","store_id":"s1","currency":"mc","amount":"320","confirmed":true})
        );
        let free = StorePrice {
            currency: "mc".into(),
            amount: 0,
        };
        assert!(PendingPurchase::for_offer(&offer, &free).is_none());
    }

    #[test]
    fn search_omits_empty_fields() {
        let search = StoreSearch {
            term: "castle".into(),
            ..StoreSearch::default()
        };
        assert_eq!(
            serde_json::to_value(&search).expect("encode"),
            serde_json::json!({"term":"castle"})
        );
    }

    #[test]
    fn parses_page_offer_and_purchase_results() {
        let page = br#"{"jsonrpc":"2.0","id":1,"result":{"schema_version":1,"page":{"id":"store",
            "rows":[{"title":"Featured","offers":[{"id":"o1","title":"Alpha","owned":true,
            "prices":[{"currency":"mc","amount":320}],"future_field":1}]}],"inventory_version":"e1"}}}"#;
        let body: PageBody = parse_response(page).expect("page");
        let offer = &body.page.rows[0].offers[0];
        assert!(offer.owned && offer.prices[0].amount == 320 && offer.thumbnail_url.is_none());

        let detail = br#"{"jsonrpc":"2.0","id":1,"result":{"schema_version":1,"offer":{"id":"o1",
            "title":"Alpha","owned":false,"description":"D","screenshot_urls":["https://x.test/a.png"]}}}"#;
        let body: OfferBody = parse_response(detail).expect("offer");
        assert_eq!(body.offer.offer.id, "o1");
        assert_eq!(body.offer.screenshot_urls.len(), 1);

        let outcome =
            br#"{"jsonrpc":"2.0","id":1,"result":{"schema_version":1,"status":"price_mismatch",
            "http_status":422,"marketplace_error_code":1234,"correlation_id":"c1"}}"#;
        let outcome: PurchaseOutcome = parse_response(outcome).expect("outcome");
        assert_eq!(outcome.status, PurchaseStatus::PriceMismatch);
        assert_eq!(outcome.marketplace_error_code, 1234);
        assert!(!outcome.replayed);
    }

    #[test]
    fn parses_a_row_continuation_and_its_next_slice() {
        let page = br#"{"jsonrpc":"2.0","id":1,"result":{"schema_version":1,"page":{"id":"store",
            "rows":[{"offers":[],"continuation":"t1"}]}}}"#;
        let body: PageBody = parse_response(page).expect("page");
        assert_eq!(body.page.rows[0].continuation.as_deref(), Some("t1"));
        let more = br#"{"jsonrpc":"2.0","id":1,"result":{"schema_version":1,
            "offers":[{"id":"o","title":"T"}],"continuation":"t2"}}"#;
        let more: StoreRowMore = parse_response(more).expect("more");
        assert_eq!(
            (more.offers.len(), more.continuation.as_deref()),
            (1, Some("t2"))
        );
    }

    #[test]
    fn parses_a_cached_image_reply() {
        let reply = br#"{"jsonrpc":"2.0","id":1,"result":{"schema_version":1,
            "image":{"path":"/cache/a.png","content_type":"image/png"}}}"#;
        let body: ImageBody = parse_response(reply).expect("image");
        assert_eq!(body.image.content_type, "image/png");
    }

    #[test]
    fn parses_balances_and_entitlement_windows() {
        let balances = br#"{"jsonrpc":"2.0","id":1,"result":{"schema_version":1,"balances":[{"currency":"mc","amount":1500}]}}"#;
        let body: BalanceBody = parse_response(balances).expect("balances");
        assert_eq!(body.balances[0].amount, 1500);
        let window = br#"{"jsonrpc":"2.0","id":1,"result":{"schema_version":1,"owned":["a"],"total":3,"offset":2}}"#;
        let window: StoreEntitlements = parse_response(window).expect("window");
        assert_eq!((window.total, window.offset, window.owned.len()), (3, 2, 1));
    }
}
