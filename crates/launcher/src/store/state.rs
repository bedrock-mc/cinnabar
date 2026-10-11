//! What the store screens show, folded from worker events and player actions. Every event or action
//! may ask for requests; nothing here does I/O, so the driver owns the socket and this stays testable.

use std::collections::{HashMap, HashSet, VecDeque};

use bridge::{
    PurchaseStatus, StoreBalance, StoreOffer, StoreOfferDetail, StorePage, StorePrice, StoreSearch,
    StoreSearchResults,
};

use super::action::StoreAction;
use super::flow::{Begin, PurchaseFlow, new_purchase_id};
use super::settings::StoreSettings;
use super::snapshot::{DisplayRow, StoreSnapshot, StoreView, role_for};
use super::worker::{StoreError, StoreEvent, StoreRequest};

const MAX_IMAGES_IN_FLIGHT: usize = 64;
const MAX_IMAGE_FILES: usize = 256;
const MAX_FAILED_IMAGES: usize = 512;
const MAX_OWNED: usize = 100_000;
const MAX_ROW_OFFERS: usize = 400;
const INVENTORY_LOOKUPS: usize = 24;
const MAX_CACHED_DETAILS: usize = 256;

pub struct StoreState {
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
    reconciliation: Option<(bool, bool)>,
    inventory_cursor: usize,
    pending_details: HashSet<String>,
    queued_images: VecDeque<String>,
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
    pub fn new() -> Self {
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
            reconciliation: None,
            inventory_cursor: 0,
            pending_details: HashSet::new(),
            queued_images: VecDeque::new(),
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

    pub fn set_settings(&mut self, settings: StoreSettings) {
        self.purchases_enabled = settings.purchases_enabled;
    }

    /// Whether the player backed out of the store's first screen.
    pub fn take_exit(&mut self) -> bool {
        std::mem::take(&mut self.exit)
    }

    /// Whether the presented state changed since the last call.
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    /// Start a store session: the home page, the balance and the owned content.
    pub fn open(&mut self) -> Vec<StoreRequest> {
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
                    role: role_for(row.kind.as_deref()).unwrap_or("StoreRow"),
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

    pub fn snapshot(&self) -> StoreSnapshot {
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
    pub fn act(&mut self, action: StoreAction) -> Vec<StoreRequest> {
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
                self.inventory_cursor = 0;
                self.inventory_lookups()
            }
            StoreAction::CoinWallet => Vec::new(),
            StoreAction::OpenOffer { row, index } => {
                let Some(offer) = self.offer_at(usize::from(row), usize::from(index)) else {
                    return Vec::new();
                };
                self.detail = self.details.get(&offer.id.to_ascii_lowercase()).cloned();
                self.pending_details.insert(offer.id.to_ascii_lowercase());
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
        let mut requests = Vec::new();
        while self.pending_details.len() < INVENTORY_LOOKUPS {
            let Some(id) = self.owned_order.get(self.inventory_cursor).cloned() else {
                break;
            };
            self.inventory_cursor += 1;
            if !self.details.contains_key(&id) && self.pending_details.insert(id.clone()) {
                requests.push(StoreRequest::Offer(id));
            }
        }
        requests
    }

    fn press_purchase(&mut self, offer: &StoreOffer, price: &StorePrice) -> Vec<StoreRequest> {
        if self.reconciliation.is_some() {
            return Vec::new();
        }
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
        if self.reconciliation.is_some() {
            return Vec::new();
        }
        match self.flow.confirm(new_purchase_id(), self.purchases_enabled) {
            Some(purchase) => self.dispatch(purchase),
            None => Vec::new(),
        }
    }

    /// The single place a purchase leaves the client; it re-checks the setting so no path can send
    /// while purchases are off.
    fn dispatch(&mut self, purchase: bridge::ConfirmedPurchase) -> Vec<StoreRequest> {
        if !self.purchases_enabled {
            let id = purchase.purchase_id().to_owned();
            self.flow.finish(&id, Err(StoreError::Unavailable));
            return Vec::new();
        }
        vec![StoreRequest::Purchase(purchase)]
    }

    /// A request the driver could not queue.
    pub fn refused(&mut self, request: &StoreRequest) {
        self.dirty = true;
        match request {
            StoreRequest::Offer(id) => {
                self.pending_details.remove(&id.to_ascii_lowercase());
                if self.selected.as_ref().is_some_and(|offer| offer.id == *id) {
                    self.failure = Some(StoreError::Unavailable);
                }
            }
            StoreRequest::Purchase(purchase) => {
                let id = purchase.purchase_id().to_owned();
                self.flow.finish(&id, Err(StoreError::Unavailable));
            }
            StoreRequest::RowMore { row, .. } => {
                self.pending_rows.remove(row);
            }
            StoreRequest::Image(url) => {
                self.pending_images.remove(url);
                self.fail_image(url.clone());
            }
            StoreRequest::Home(_) => self.loading_page = false,
            StoreRequest::Search(_) => self.loading_search = false,
            _ => {}
        }
    }

    /// Fold one worker event in; returns the follow-up requests it calls for.
    pub fn apply(&mut self, event: StoreEvent) -> Vec<StoreRequest> {
        self.dirty = true;
        match event {
            StoreEvent::Page(Ok(mut page)) => {
                // Dropped here, not when drawn, so row indices stay those of `page.rows`.
                page.rows
                    .retain(|row| role_for(row.kind.as_deref()).is_some());
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
                self.pending_details
                    .remove(&detail.offer.id.to_ascii_lowercase());
                self.failure = None;
                if self.details.len() >= MAX_CACHED_DETAILS {
                    self.details.clear();
                }
                self.details
                    .insert(detail.offer.id.to_ascii_lowercase(), detail.clone());
                // The offer page's art jumps the queue ahead of the page's remaining thumbnails.
                let art = detail.offer.thumbnail_url.iter().cloned();
                let shots = detail.screenshot_urls.iter().take(8).cloned();
                let mut requests = self.image_urls(art.chain(shots), true);
                if self
                    .selected
                    .as_ref()
                    .is_some_and(|s| s.id == detail.offer.id)
                {
                    self.detail = Some(detail);
                }
                if self.view == StoreView::Inventory {
                    requests.extend(self.inventory_lookups());
                }
                requests
            }
            StoreEvent::Balance(Ok(balances)) => {
                self.balances = balances;
                self.reconcile(true, false);
                Vec::new()
            }
            StoreEvent::Entitlements {
                offset,
                result: Ok(window),
            } => {
                if offset == 0 {
                    self.owned.clear();
                    self.owned_order.clear();
                    self.inventory_cursor = 0;
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
                } else {
                    self.reconcile(false, true);
                    if self.view == StoreView::Inventory {
                        self.loading_page = false;
                        self.inventory_lookups()
                    } else {
                        Vec::new()
                    }
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
                let matching = matches!(&self.flow, PurchaseFlow::InProgress { purchase_id: active, .. } if *active == purchase_id);
                let uncertain = matching
                    && (matches!(&result, Ok(outcome) if outcome.status == PurchaseStatus::Unknown)
                        || matches!(&result, Err(StoreError::Busy)));
                if uncertain {
                    self.reconciliation = Some((false, false));
                }
                let refresh = uncertain
                    || matches!(
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
                self.image_urls(std::iter::empty(), false)
            }
            StoreEvent::OfferFailed { id, error } => {
                self.pending_details.remove(&id.to_ascii_lowercase());
                self.failure = Some(error);
                if self.view == StoreView::Inventory {
                    self.inventory_lookups()
                } else {
                    Vec::new()
                }
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

    /// Releases uncertain purchase admission only after both authoritative reads succeed.
    fn reconcile(&mut self, balance: bool, entitlements: bool) {
        if let Some((got_balance, got_entitlements)) = &mut self.reconciliation {
            *got_balance |= balance;
            *got_entitlements |= entitlements;
            if *got_balance && *got_entitlements {
                self.reconciliation = None;
            }
        }
    }

    fn fail_image(&mut self, url: String) {
        if self.failed_images.len() >= MAX_FAILED_IMAGES {
            self.failed_images.clear();
        }
        self.failed_images.insert(url);
    }

    /// Image fetches for offers whose thumbnail is neither cached, in flight nor known bad.
    fn image_requests<'a>(
        &mut self,
        offers: impl Iterator<Item = &'a StoreOffer>,
    ) -> Vec<StoreRequest> {
        let urls: Vec<String> = offers
            .filter_map(|offer| offer.thumbnail_url.clone())
            .collect();
        self.image_urls(urls.into_iter(), false)
    }

    /// Queues image fetches, ahead of those already waiting when `first`, and starts what fits.
    fn image_urls(&mut self, urls: impl Iterator<Item = String>, first: bool) -> Vec<StoreRequest> {
        let mut front = Vec::new();
        for url in urls {
            if self.image_files.contains_key(&url)
                || self.pending_images.contains(&url)
                || self.failed_images.contains(&url)
            {
                continue;
            }
            if first {
                if front.contains(&url) {
                    continue;
                }
                self.queued_images.retain(|queued| *queued != url);
                front.push(url);
            } else if !self.queued_images.contains(&url) && self.queued_images.len() < MAX_OWNED {
                self.queued_images.push_back(url);
            }
        }
        for url in front.into_iter().rev() {
            self.queued_images.push_front(url);
        }
        let mut requests = Vec::new();
        while self.pending_images.len() < MAX_IMAGES_IN_FLIGHT {
            let Some(url) = self.queued_images.pop_front() else {
                break;
            };
            if self.image_files.contains_key(&url)
                || self.failed_images.contains(&url)
                || !self.pending_images.insert(url.clone())
            {
                continue;
            }
            requests.push(StoreRequest::Image(url));
        }
        requests
    }
}

#[cfg(test)]
mod tests;
