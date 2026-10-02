//! What the store screens show, folded from worker events and player actions. Every event or action
//! may ask for requests; nothing here does I/O, so the driver owns the socket and this stays testable.

use std::collections::{HashMap, HashSet};

use bevy::prelude::Resource;
use protocol::store_control::{
    PurchaseStatus, StoreBalance, StoreOffer, StoreOfferDetail, StorePage, StorePrice, StoreSearch,
    StoreSearchResults,
};

use super::action::StoreAction;
use super::flow::{Begin, PurchaseDialog, PurchaseFlow, new_purchase_id};
use super::settings::StoreSettings;
use super::snapshot::{DisplayRow, StoreSnapshot, StoreView, role_for};
use super::worker::{StoreError, StoreEvent, StoreRequest};

const MAX_IMAGE_REQUESTS_PER_EVENT: usize = 64;
const MAX_IMAGE_FILES: usize = 256;
const MAX_FAILED_IMAGES: usize = 512;
const MAX_OWNED: usize = 100_000;
const MAX_ROW_OFFERS: usize = 400;
const INVENTORY_LOOKUPS: usize = 24;
const MAX_CACHED_DETAILS: usize = 256;

#[derive(Resource)]
pub(crate) struct StoreState {
    view: StoreView,
    back: Vec<StoreView>,
    page: Option<StorePage>,
    search: Option<StoreSearchResults>,
    search_query: StoreSearch,
    search_appending: bool,
    selected: Option<StoreOffer>,
    detail: Option<StoreOfferDetail>,
    details: HashMap<String, StoreOfferDetail>,
    balances: Vec<StoreBalance>,
    flow: PurchaseFlow,
    failure: Option<StoreError>,
    loading_page: bool,
    loading_search: bool,
    owned: HashSet<String>,
    owned_order: Vec<String>,
    pending_rows: HashSet<usize>,
    pending_images: HashSet<String>,
    failed_images: HashSet<String>,
    image_files: HashMap<String, String>,
    purchases_enabled: bool,
    exit: bool,
    dirty: bool,
}

impl Default for StoreState {
    fn default() -> Self {
        Self::new()
    }
}

impl StoreState {
    pub(crate) fn new() -> Self {
        Self {
            view: StoreView::Home,
            back: Vec::new(),
            page: None,
            search: None,
            search_query: StoreSearch::default(),
            search_appending: false,
            selected: None,
            detail: None,
            details: HashMap::new(),
            balances: Vec::new(),
            flow: PurchaseFlow::Idle,
            failure: None,
            loading_page: false,
            loading_search: false,
            owned: HashSet::new(),
            owned_order: Vec::new(),
            pending_rows: HashSet::new(),
            pending_images: HashSet::new(),
            failed_images: HashSet::new(),
            image_files: HashMap::new(),
            purchases_enabled: false,
            exit: false,
            dirty: true,
        }
    }

    pub(crate) fn set_settings(&mut self, settings: StoreSettings) {
        self.purchases_enabled = settings.purchases_enabled;
    }

    /// Whether the player backed out of the store's first screen.
    pub(crate) fn take_exit(&mut self) -> bool {
        std::mem::take(&mut self.exit)
    }

    /// Whether the presented state changed since the last call.
    pub(crate) fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    /// Start a store session: the home page, the balance and the owned content.
    pub(crate) fn open(&mut self) -> Vec<StoreRequest> {
        self.view = StoreView::Home;
        self.back.clear();
        self.flow = PurchaseFlow::Idle;
        self.failure = None;
        self.loading_page = true;
        self.dirty = true;
        vec![
            StoreRequest::Home(None),
            StoreRequest::Balance,
            StoreRequest::Entitlements {
                offset: 0,
                refresh: false,
            },
        ]
    }

    /// The offers each screen row shows, with ownership merged in.
    fn rows(&self) -> Vec<DisplayRow> {
        let mut rows = match self.view {
            StoreView::Home => self
                .page
                .iter()
                .flat_map(|page| &page.rows)
                .map(|row| DisplayRow {
                    id: row.id.clone(),
                    title: row.title.clone().unwrap_or_default(),
                    role: role_for(row.kind.as_deref()),
                    offers: row.offers.clone(),
                    continuation: row.continuation.clone(),
                })
                .collect(),
            StoreView::Search => self
                .search
                .iter()
                .map(|results| DisplayRow {
                    id: None,
                    title: String::new(),
                    role: "GridList",
                    offers: results.offers.clone(),
                    continuation: results.continuation.clone(),
                })
                .collect(),
            StoreView::Inventory => vec![DisplayRow {
                id: None,
                title: String::new(),
                role: "GridList",
                offers: self
                    .owned_order
                    .iter()
                    .filter_map(|id| self.details.get(id))
                    .map(|detail| detail.offer.clone())
                    .collect(),
                continuation: None,
            }],
            StoreView::Detail => Vec::new(),
        };
        for row in &mut rows {
            for offer in &mut row.offers {
                offer.owned = offer.owned || self.owned.contains(&offer.id.to_ascii_lowercase());
            }
        }
        rows
    }

    pub(crate) fn snapshot(&self) -> StoreSnapshot {
        let detail = (self.view == StoreView::Detail)
            .then(|| {
                self.detail.clone().or_else(|| {
                    self.selected.clone().map(|offer| StoreOfferDetail {
                        offer,
                        description: None,
                        screenshot_urls: Vec::new(),
                        display_version: None,
                        platforms: Vec::new(),
                    })
                })
            })
            .flatten()
            .map(|mut detail| {
                detail.offer.owned = detail.offer.owned
                    || self.owned.contains(&detail.offer.id.to_ascii_lowercase());
                detail
            });
        StoreSnapshot {
            view: self.view,
            rows: self.rows(),
            detail,
            balance: self.balance(),
            loading: match self.view {
                StoreView::Home | StoreView::Inventory => self.loading_page,
                StoreView::Search => self.loading_search,
                StoreView::Detail => self.detail.is_none() && self.failure.is_none(),
            },
            failure: self.failure,
            flow: self.flow.clone(),
            images: self.image_files.clone(),
            owned_total: self.owned.len(),
            search_term: self.search_query.term.clone(),
        }
    }

    /// The balance of the currency the store prices in: the first price seen, else the first balance.
    fn balance(&self) -> Option<i64> {
        let priced = self
            .page
            .iter()
            .flat_map(|page| &page.rows)
            .flat_map(|row| &row.offers)
            .chain(self.selected.iter())
            .find_map(|offer| offer.prices.first())
            .map(|price| price.currency.as_str());
        priced
            .and_then(|currency| self.balances.iter().find(|b| b.currency == currency))
            .or_else(|| self.balances.first())
            .map(|balance| balance.amount)
    }

    /// The currently displayed offer at (`row`, `index`).
    fn offer_at(&self, row: usize, index: usize) -> Option<StoreOffer> {
        self.rows().get(row)?.offers.get(index).cloned()
    }

    /// Apply a player action; returns the requests it calls for.
    pub(crate) fn act(&mut self, action: StoreAction) -> Vec<StoreRequest> {
        self.dirty = true;
        match action {
            StoreAction::Open => self.open(),
            StoreAction::Back => {
                if !self.flow.is_idle() && !matches!(self.flow, PurchaseFlow::InProgress { .. }) {
                    self.flow.dismiss();
                } else if let Some(previous) = self.back.pop() {
                    self.view = previous;
                } else {
                    self.exit = true;
                }
                Vec::new()
            }
            StoreAction::Home => {
                self.view = StoreView::Home;
                self.back.clear();
                Vec::new()
            }
            StoreAction::OpenSearch => {
                self.enter(StoreView::Search);
                if self.search.is_some() {
                    return Vec::new();
                }
                self.loading_search = true;
                vec![StoreRequest::Search(self.search_query.clone())]
            }
            StoreAction::Inventory => {
                self.enter(StoreView::Inventory);
                self.inventory_lookups()
            }
            StoreAction::CoinWallet => Vec::new(),
            StoreAction::OpenOffer { row, index } => {
                let Some(offer) = self.offer_at(usize::from(row), usize::from(index)) else {
                    return Vec::new();
                };
                self.detail = self.details.get(&offer.id).cloned();
                let request = StoreRequest::Offer(offer.id.clone());
                self.selected = Some(offer);
                self.enter(StoreView::Detail);
                vec![request]
            }
            StoreAction::ShowMore { row } => self.show_more(usize::from(row)),
            StoreAction::Buy => {
                let Some(offer) = self
                    .detail
                    .as_ref()
                    .map(|detail| detail.offer.clone())
                    .or_else(|| self.selected.clone())
                else {
                    return Vec::new();
                };
                let owned = offer.owned || self.owned.contains(&offer.id.to_ascii_lowercase());
                let Some(price) = offer.prices.iter().find(|price| price.amount > 0).cloned()
                else {
                    return Vec::new();
                };
                if owned {
                    return Vec::new();
                }
                self.press_purchase(&offer, &price)
            }
            StoreAction::ModalPrimary => {
                if matches!(self.flow, PurchaseFlow::Confirming { .. }) {
                    return self.confirm_purchase();
                }
                self.flow.dismiss();
                Vec::new()
            }
            StoreAction::ModalSecondary | StoreAction::ModalDismiss => {
                self.flow.dismiss();
                Vec::new()
            }
        }
    }

    fn enter(&mut self, view: StoreView) {
        if self.view != view {
            self.back.push(self.view);
            self.view = view;
        }
    }

    fn show_more(&mut self, row: usize) -> Vec<StoreRequest> {
        match self.view {
            StoreView::Search => {
                let Some(token) = self.search.as_ref().and_then(|r| r.continuation.clone()) else {
                    return Vec::new();
                };
                if self.loading_search {
                    return Vec::new();
                }
                self.loading_search = true;
                self.search_appending = true;
                let mut query = self.search_query.clone();
                query.continuation = token;
                vec![StoreRequest::Search(query)]
            }
            StoreView::Home => {
                let Some(token) = self
                    .page
                    .as_ref()
                    .and_then(|page| page.rows.get(row))
                    .and_then(|row| row.continuation.clone())
                else {
                    return Vec::new();
                };
                if !self.pending_rows.insert(row) {
                    return Vec::new();
                }
                vec![StoreRequest::RowMore {
                    row,
                    continuation: token,
                }]
            }
            _ => Vec::new(),
        }
    }

    /// Detail lookups for owned ids not yet resolved, so the inventory can name them.
    fn inventory_lookups(&mut self) -> Vec<StoreRequest> {
        self.owned_order
            .iter()
            .filter(|id| !self.details.contains_key(*id))
            .take(INVENTORY_LOOKUPS)
            .map(|id| StoreRequest::Offer(id.clone()))
            .collect()
    }

    fn press_purchase(&mut self, offer: &StoreOffer, price: &StorePrice) -> Vec<StoreRequest> {
        let id = new_purchase_id();
        match self.flow.begin(
            offer,
            price,
            &self.balances,
            None,
            id,
            self.purchases_enabled,
        ) {
            Begin::Send(purchase) => self.dispatch(purchase),
            Begin::Showing | Begin::Ignored => Vec::new(),
        }
    }

    fn confirm_purchase(&mut self) -> Vec<StoreRequest> {
        match self.flow.confirm(new_purchase_id(), self.purchases_enabled) {
            Some(purchase) => self.dispatch(purchase),
            None => Vec::new(),
        }
    }

    /// The single place a purchase leaves the client; it re-checks the setting so no path can send
    /// while purchases are off.
    fn dispatch(
        &mut self,
        purchase: protocol::store_control::ConfirmedPurchase,
    ) -> Vec<StoreRequest> {
        if !self.purchases_enabled {
            let id = purchase.purchase_id().to_owned();
            self.flow.finish(&id, Err(StoreError::Unavailable));
            return Vec::new();
        }
        vec![StoreRequest::Purchase(purchase)]
    }

    /// A request the driver could not queue.
    pub(crate) fn refused(&mut self, request: &StoreRequest) {
        self.dirty = true;
        match request {
            StoreRequest::Purchase(purchase) => {
                let id = purchase.purchase_id().to_owned();
                self.flow.finish(&id, Err(StoreError::Unavailable));
            }
            StoreRequest::RowMore { row, .. } => {
                self.pending_rows.remove(row);
            }
            StoreRequest::Image(url) => {
                self.pending_images.remove(url);
            }
            StoreRequest::Home(_) => self.loading_page = false,
            StoreRequest::Search(_) => self.loading_search = false,
            _ => {}
        }
    }

    /// Fold one worker event in; returns the follow-up requests it calls for.
    pub(crate) fn apply(&mut self, event: StoreEvent) -> Vec<StoreRequest> {
        self.dirty = true;
        match event {
            StoreEvent::Page(Ok(page)) => {
                self.failure = None;
                self.loading_page = false;
                self.pending_rows.clear();
                let requests = self.image_requests(page.rows.iter().flat_map(|row| &row.offers));
                self.page = Some(page);
                requests
            }
            StoreEvent::Search(Ok(results)) => {
                self.failure = None;
                self.loading_search = false;
                let merged = if std::mem::take(&mut self.search_appending) {
                    let mut all = self.search.take().unwrap_or(StoreSearchResults {
                        offers: Vec::new(),
                        continuation: None,
                        truncated: false,
                    });
                    all.offers.extend(results.offers);
                    all.offers.truncate(MAX_ROW_OFFERS);
                    all.continuation = results.continuation;
                    all
                } else {
                    results
                };
                let requests = self.image_requests(merged.offers.iter());
                self.search = Some(merged);
                requests
            }
            StoreEvent::Offer(Ok(detail)) => {
                let detail = *detail;
                self.failure = None;
                if self.details.len() >= MAX_CACHED_DETAILS {
                    self.details.clear();
                }
                self.details.insert(detail.offer.id.clone(), detail.clone());
                let mut requests = self.image_requests(std::iter::once(&detail.offer));
                requests.extend(self.screenshot_requests(&detail));
                if self
                    .selected
                    .as_ref()
                    .is_some_and(|s| s.id == detail.offer.id)
                {
                    self.detail = Some(detail);
                }
                requests
            }
            StoreEvent::Balance(Ok(balances)) => {
                self.balances = balances;
                Vec::new()
            }
            StoreEvent::Entitlements {
                offset,
                result: Ok(window),
            } => {
                if offset == 0 {
                    self.owned.clear();
                    self.owned_order.clear();
                }
                let taken = window.owned.len() as u32;
                for id in window.owned {
                    if self.owned.len() < MAX_OWNED && self.owned.insert(id.to_ascii_lowercase()) {
                        self.owned_order.push(id.to_ascii_lowercase());
                    }
                }
                if taken > 0 && window.offset + taken < window.total {
                    vec![StoreRequest::Entitlements {
                        offset: window.offset + taken,
                        refresh: false,
                    }]
                } else if self.view == StoreView::Inventory {
                    self.loading_page = false;
                    self.inventory_lookups()
                } else {
                    Vec::new()
                }
            }
            StoreEvent::RowMore { row, result } => {
                self.pending_rows.remove(&row);
                match result {
                    Ok(more) => {
                        let requests = self.image_requests(more.offers.iter());
                        if let Some(target) =
                            self.page.as_mut().and_then(|page| page.rows.get_mut(row))
                        {
                            target.offers.extend(more.offers);
                            target.offers.truncate(MAX_ROW_OFFERS);
                            target.continuation = more.continuation;
                        }
                        requests
                    }
                    Err(error) => {
                        self.failure = Some(error);
                        Vec::new()
                    }
                }
            }
            StoreEvent::Purchase {
                purchase_id,
                result,
            } => {
                let refresh = matches!(
                    &result,
                    Ok(outcome) if !matches!(outcome.status, PurchaseStatus::PriceMismatch)
                );
                self.flow.finish(&purchase_id, result);
                if refresh {
                    // Re-read what the purchase may have changed instead of assuming it.
                    vec![
                        StoreRequest::Balance,
                        StoreRequest::Entitlements {
                            offset: 0,
                            refresh: true,
                        },
                        StoreRequest::Home(None),
                    ]
                } else {
                    vec![StoreRequest::Balance]
                }
            }
            StoreEvent::Image { url, result } => {
                self.pending_images.remove(&url);
                match result {
                    Ok(path) => {
                        if self.image_files.len() >= MAX_IMAGE_FILES {
                            self.image_files.clear();
                        }
                        self.image_files
                            .insert(url, path.to_string_lossy().into_owned());
                    }
                    Err(_) => self.fail_image(url),
                }
                Vec::new()
            }
            StoreEvent::Page(Err(error)) => {
                self.loading_page = false;
                self.failure = Some(error);
                Vec::new()
            }
            StoreEvent::Search(Err(error)) => {
                self.loading_search = false;
                self.search_appending = false;
                self.failure = Some(error);
                Vec::new()
            }
            StoreEvent::Offer(Err(error))
            | StoreEvent::Balance(Err(error))
            | StoreEvent::Entitlements {
                result: Err(error), ..
            } => {
                self.failure = Some(error);
                Vec::new()
            }
        }
    }

    fn fail_image(&mut self, url: String) {
        if self.failed_images.len() >= MAX_FAILED_IMAGES {
            self.failed_images.clear();
        }
        self.failed_images.insert(url);
    }

    fn screenshot_requests(&mut self, detail: &StoreOfferDetail) -> Vec<StoreRequest> {
        let urls: Vec<String> = detail.screenshot_urls.iter().take(8).cloned().collect();
        self.image_urls(urls.into_iter())
    }

    /// Image fetches for offers whose thumbnail is neither cached, in flight nor known bad.
    fn image_requests<'a>(
        &mut self,
        offers: impl Iterator<Item = &'a StoreOffer>,
    ) -> Vec<StoreRequest> {
        let urls: Vec<String> = offers
            .filter_map(|offer| offer.thumbnail_url.clone())
            .collect();
        self.image_urls(urls.into_iter())
    }

    fn image_urls(&mut self, urls: impl Iterator<Item = String>) -> Vec<StoreRequest> {
        let mut requests = Vec::new();
        for url in urls {
            if requests.len() >= MAX_IMAGE_REQUESTS_PER_EVENT
                || self.image_files.contains_key(&url)
                || self.pending_images.contains(&url)
                || self.failed_images.contains(&url)
            {
                continue;
            }
            self.pending_images.insert(url.clone());
            requests.push(StoreRequest::Image(url));
        }
        requests
    }
}

#[cfg(test)]
mod tests {
    use protocol::store_control::{PurchaseOutcome, StoreEntitlements, StoreRow, StoreRowMore};

    use super::*;

    fn offer(id: &str, thumbnail: Option<&str>, price: Option<i64>) -> StoreOffer {
        StoreOffer {
            id: id.into(),
            title: id.into(),
            creator: None,
            content_type: None,
            thumbnail_url: thumbnail.map(str::to_owned),
            store_id: None,
            prices: price
                .map(|amount| {
                    vec![StorePrice {
                        currency: "mc".into(),
                        amount,
                    }]
                })
                .unwrap_or_default(),
            rating: None,
            tags: vec![],
            owned: false,
        }
    }

    fn page(rows: Vec<Vec<StoreOffer>>) -> StorePage {
        StorePage {
            id: "store".into(),
            rows: rows
                .into_iter()
                .map(|offers| StoreRow {
                    id: None,
                    title: Some("Featured".into()),
                    kind: None,
                    offers,
                    continuation: Some("more-1".into()),
                })
                .collect(),
            inventory_version: None,
            truncated: false,
        }
    }

    fn outcome(status: PurchaseStatus) -> PurchaseOutcome {
        PurchaseOutcome {
            status,
            http_status: 200,
            marketplace_error_code: 0,
            correlation_id: "c".into(),
            inventory_version: None,
            replayed: false,
        }
    }

    fn loaded(enabled: bool) -> StoreState {
        let mut state = StoreState::new();
        state.set_settings(StoreSettings {
            purchases_enabled: enabled,
        });
        state.apply(StoreEvent::Page(Ok(page(vec![vec![
            offer("a", Some("https://x.test/a.png"), Some(320)),
            offer("b", None, None),
        ]]))));
        state.apply(StoreEvent::Balance(Ok(vec![StoreBalance {
            currency: "mc".into(),
            amount: 1000,
        }])));
        state
    }

    #[test]
    fn a_page_asks_for_each_thumbnail_once_and_failures_are_not_retried() {
        let mut state = StoreState::new();
        let shown = page(vec![vec![
            offer("a", Some("https://x.test/a.png"), None),
            offer("b", Some("https://x.test/a.png"), None),
            offer("c", None, None),
        ]]);
        assert_eq!(state.apply(StoreEvent::Page(Ok(shown.clone()))).len(), 1);
        assert!(state.apply(StoreEvent::Page(Ok(shown.clone()))).is_empty());
        state.apply(StoreEvent::Image {
            url: "https://x.test/a.png".into(),
            result: Err(StoreError::Rejected),
        });
        assert!(state.apply(StoreEvent::Page(Ok(shown))).is_empty());
    }

    #[test]
    fn an_offer_card_opens_the_detail_and_a_disabled_buy_only_shows_the_notice() {
        let mut state = loaded(false);
        let requests = state.act(StoreAction::OpenOffer { row: 0, index: 0 });
        assert!(matches!(requests.as_slice(), [StoreRequest::Offer(id)] if id == "a"));
        assert_eq!(state.snapshot().view, StoreView::Detail);
        assert!(
            state.snapshot().detail.is_some(),
            "the selected card renders before the detail arrives"
        );
        let sent = state.act(StoreAction::Buy);
        assert!(sent.is_empty(), "purchases are off: {sent:?}");
        assert_eq!(state.flow, PurchaseFlow::Done(PurchaseDialog::Disabled));
        state.act(StoreAction::ModalPrimary);
        assert!(state.flow.is_idle());
    }

    #[test]
    fn buy_uses_the_displayed_detail_offer() {
        let mut state = loaded(true);
        state.act(StoreAction::OpenOffer { row: 0, index: 0 });
        let mut detail_offer = offer("a", None, Some(640));
        detail_offer.title = "Current title".into();
        detail_offer.store_id = Some("current-store".into());
        state.apply(StoreEvent::Offer(Ok(Box::new(StoreOfferDetail {
            offer: detail_offer.clone(),
            description: None,
            screenshot_urls: vec![],
            display_version: None,
            platforms: vec![],
        }))));
        assert_eq!(state.snapshot().detail.unwrap().offer, detail_offer);
        let sent = state.act(StoreAction::Buy);
        let [StoreRequest::Purchase(purchase)] = sent.as_slice() else {
            panic!("expected a purchase, got {sent:?}");
        };
        let expected = protocol::store_control::PendingPurchase::for_offer(
            &detail_offer,
            &detail_offer.prices[0],
        )
        .unwrap()
        .confirm(purchase.purchase_id().to_owned());
        assert_eq!(purchase, &expected);
        assert!(
            matches!(&state.flow, PurchaseFlow::InProgress { offer_title, .. }
            if offer_title == "Current title")
        );
    }

    #[test]
    fn an_enabled_buy_sends_one_confirmed_purchase_and_refreshes_after_it_completes() {
        let mut state = loaded(true);
        state.act(StoreAction::OpenOffer { row: 0, index: 0 });
        let sent = state.act(StoreAction::Buy);
        let [StoreRequest::Purchase(purchase)] = sent.as_slice() else {
            panic!("expected one purchase, got {sent:?}");
        };
        let id = purchase.purchase_id().to_owned();
        assert!(
            state.act(StoreAction::Buy).is_empty(),
            "a second press while running is ignored"
        );
        let follow = state.apply(StoreEvent::Purchase {
            purchase_id: id,
            result: Ok(outcome(PurchaseStatus::Purchased)),
        });
        assert!(follow.iter().any(|r| matches!(
            r,
            StoreRequest::Entitlements {
                offset: 0,
                refresh: true
            }
        )));
        assert!(follow.iter().any(|r| matches!(r, StoreRequest::Balance)));
        assert!(matches!(
            state.flow,
            PurchaseFlow::Done(PurchaseDialog::Success { .. })
        ));
    }

    #[test]
    fn a_price_refusal_only_rereads_the_balance_and_an_uncovered_price_never_sends() {
        let mut state = loaded(true);
        state.act(StoreAction::OpenOffer { row: 0, index: 0 });
        let sent = state.act(StoreAction::Buy);
        let [StoreRequest::Purchase(purchase)] = sent.as_slice() else {
            panic!("expected a purchase");
        };
        let follow = state.apply(StoreEvent::Purchase {
            purchase_id: purchase.purchase_id().to_owned(),
            result: Ok(outcome(PurchaseStatus::PriceMismatch)),
        });
        assert_eq!(follow.len(), 1);
        state.act(StoreAction::ModalDismiss);
        state.apply(StoreEvent::Balance(Ok(vec![StoreBalance {
            currency: "mc".into(),
            amount: 5,
        }])));
        assert!(state.act(StoreAction::Buy).is_empty());
        assert!(matches!(
            state.flow,
            PurchaseFlow::Done(PurchaseDialog::InsufficientFunds { missing: 315 })
        ));
    }

    #[test]
    fn show_more_asks_once_per_row_and_appends_the_answer() {
        let mut state = loaded(false);
        let first = state.act(StoreAction::ShowMore { row: 0 });
        assert!(
            matches!(first.as_slice(), [StoreRequest::RowMore { row: 0, continuation }] if continuation == "more-1")
        );
        assert!(
            state.act(StoreAction::ShowMore { row: 0 }).is_empty(),
            "already loading"
        );
        state.apply(StoreEvent::RowMore {
            row: 0,
            result: Ok(StoreRowMore {
                offers: vec![offer("c", None, None)],
                continuation: None,
            }),
        });
        let rows = state.snapshot().rows;
        assert_eq!(rows[0].offers.len(), 3);
        assert_eq!(rows[0].continuation, None);
        assert!(
            state.act(StoreAction::ShowMore { row: 0 }).is_empty(),
            "no token left"
        );
    }

    #[test]
    fn search_paginates_by_appending_and_back_returns_home() {
        let mut state = loaded(false);
        let start = state.act(StoreAction::OpenSearch);
        assert!(matches!(start.as_slice(), [StoreRequest::Search(_)]));
        state.apply(StoreEvent::Search(Ok(StoreSearchResults {
            offers: vec![offer("s1", None, None)],
            continuation: Some("p2".into()),
            truncated: false,
        })));
        let more = state.act(StoreAction::ShowMore { row: 0 });
        assert!(matches!(more.as_slice(), [StoreRequest::Search(q)] if q.continuation == "p2"));
        state.apply(StoreEvent::Search(Ok(StoreSearchResults {
            offers: vec![offer("s2", None, None)],
            continuation: None,
            truncated: false,
        })));
        assert_eq!(state.snapshot().rows[0].offers.len(), 2);
        state.act(StoreAction::Back);
        assert_eq!(state.snapshot().view, StoreView::Home);
    }

    #[test]
    fn entitlement_windows_chain_and_mark_offers_owned() {
        let mut state = loaded(false);
        let next = state.apply(StoreEvent::Entitlements {
            offset: 0,
            result: Ok(StoreEntitlements {
                owned: vec!["A".into()],
                total: 2,
                offset: 0,
                inventory_version: None,
            }),
        });
        assert!(matches!(
            next.as_slice(),
            [StoreRequest::Entitlements {
                offset: 1,
                refresh: false
            }]
        ));
        assert!(state.snapshot().rows[0].offers[0].owned);
        assert!(state.act(StoreAction::OpenOffer { row: 0, index: 0 }).len() == 1);
        assert!(
            state.act(StoreAction::Buy).is_empty(),
            "an owned offer has nothing to buy"
        );
    }

    #[test]
    fn back_from_the_first_screen_asks_to_leave_the_store() {
        let mut state = loaded(false);
        assert!(!state.take_exit());
        state.act(StoreAction::Back);
        assert!(state.take_exit());
        assert!(!state.take_exit());
    }

    #[test]
    fn read_failures_surface_and_clear_on_success() {
        let mut state = StoreState::new();
        state.apply(StoreEvent::Search(Err(StoreError::SignedOut)));
        assert_eq!(state.snapshot().failure, Some(StoreError::SignedOut));
        state.apply(StoreEvent::Search(Ok(StoreSearchResults {
            offers: vec![],
            continuation: None,
            truncated: false,
        })));
        assert_eq!(state.snapshot().failure, None);
    }

    #[test]
    fn a_refused_purchase_request_resolves_the_flow_instead_of_hanging() {
        let mut state = loaded(true);
        state.act(StoreAction::OpenOffer { row: 0, index: 0 });
        let sent = state.act(StoreAction::Buy);
        let [request] = sent.as_slice() else {
            panic!("expected one request");
        };
        state.refused(request);
        assert!(matches!(
            state.flow,
            PurchaseFlow::Done(PurchaseDialog::Failed { .. })
        ));
    }
}
