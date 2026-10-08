use protocol::store_control::{PurchaseOutcome, StoreEntitlements, StoreRow, StoreRowMore};

use super::*;
use crate::store::flow::PurchaseDialog;

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
    let expected =
        protocol::store_control::PendingPurchase::for_offer(&detail_offer, &detail_offer.prices[0])
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

#[test]
fn review_pending_purchase_requires_successful_reconciliation_before_retry() {
    for busy in [false, true] {
        let mut state = loaded(true);
        state.act(StoreAction::OpenOffer { row: 0, index: 0 });
        let requests = state.act(StoreAction::Buy);
        let [StoreRequest::Purchase(purchase)] = requests.as_slice() else {
            panic!("purchase");
        };
        let id = purchase.purchase_id().to_owned();
        let requests = state.apply(StoreEvent::Purchase {
            purchase_id: id,
            result: if busy {
                Err(StoreError::Busy)
            } else {
                Ok(outcome(PurchaseStatus::Unknown))
            },
        });
        assert!(
            requests
                .iter()
                .any(|request| matches!(request, StoreRequest::Entitlements { refresh: true, .. }))
        );
        state.act(StoreAction::ModalDismiss);
        assert!(state.act(StoreAction::Buy).is_empty());
        state.apply(StoreEvent::Balance(Ok(vec![StoreBalance {
            currency: "mc".into(),
            amount: 1000,
        }])));
        assert!(state.act(StoreAction::Buy).is_empty());
        state.apply(StoreEvent::Entitlements {
            offset: 0,
            result: Ok(StoreEntitlements {
                owned: vec![],
                offset: 0,
                total: 0,
                inventory_version: None,
            }),
        });
        assert!(matches!(
            state.act(StoreAction::Buy).as_slice(),
            [StoreRequest::Purchase(_)]
        ));
    }
}

#[test]
fn review_background_inventory_and_image_batches_are_replenished() {
    let mut state = loaded(false);
    state.owned_order = (0..=INVENTORY_LOOKUPS)
        .map(|index| format!("owned-{index}"))
        .collect();
    let first = state.act(StoreAction::Inventory);
    assert_eq!(first.len(), INVENTORY_LOOKUPS);
    let followup = state.apply(StoreEvent::Offer(Ok(Box::new(StoreOfferDetail {
        offer: offer("owned-0", None, None),
        description: None,
        screenshot_urls: vec![],
        display_version: None,
        platforms: vec![],
    }))));
    assert!(
        followup
            .iter()
            .any(|request| matches!(request, StoreRequest::Offer(id) if id == &format!("owned-{INVENTORY_LOOKUPS}")))
    );
    let mut state = StoreState::new();
    let urls = (0..=MAX_IMAGES_IN_FLIGHT).map(|index| format!("https://x.test/{index}.png"));
    assert_eq!(state.image_urls(urls, false).len(), MAX_IMAGES_IN_FLIGHT);
    let followup = state.apply(StoreEvent::Image {
        url: "https://x.test/0.png".into(),
        result: Err(StoreError::Unavailable),
    });
    assert!(followup.iter().any(
        |request| matches!(request, StoreRequest::Image(url) if url == &format!("https://x.test/{MAX_IMAGES_IN_FLIGHT}.png"))
    ));
}

#[test]
fn review_refused_detail_does_not_leave_the_screen_loading() {
    let mut state = loaded(false);
    let requests = state.act(StoreAction::OpenOffer { row: 0, index: 0 });
    state.refused(&requests[0]);
    assert!(!state.snapshot().loading);
    assert_eq!(state.snapshot().failure, Some(StoreError::Unavailable));
}

#[test]
fn rows_without_a_client_factory_are_dropped_and_show_more_keeps_its_row() {
    let mut state = StoreState::new();
    let mut shown = page(vec![
        vec![offer("bundle", None, None)],
        vec![offer("a", None, Some(320))],
    ]);
    shown.rows[0].kind = Some("CoinBundleRow".into());
    shown.rows[1].kind = Some("HeroRow".into());
    shown.rows[1].continuation = Some("hero-more".into());
    state.apply(StoreEvent::Page(Ok(shown)));
    let rows = state.snapshot().rows;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (rows[0].role, rows[0].offers[0].id.as_str()),
        ("HeroRow", "a")
    );
    assert!(matches!(
        state.act(StoreAction::ShowMore { row: 0 }).as_slice(),
        [StoreRequest::RowMore { row: 0, continuation }] if continuation == "hero-more"
    ));
}

// Opening an offer queued its screenshots behind every thumbnail the home page still had waiting.
#[test]
fn offer_page_art_is_fetched_before_waiting_thumbnails() {
    let mut state = StoreState::new();
    let urls: Vec<String> = (0..80).map(|i| format!("https://x.test/{i}.jpg")).collect();
    let offers = urls
        .iter()
        .enumerate()
        .map(|(i, url)| offer(&i.to_string(), Some(url), None))
        .collect();
    let started = state.apply(StoreEvent::Page(Ok(page(vec![offers]))));
    assert!(started.len() < urls.len(), "some thumbnails wait");
    state.apply(StoreEvent::Offer(Ok(Box::new(StoreOfferDetail {
        offer: offer("0", None, None),
        description: None,
        screenshot_urls: vec!["https://x.test/shot.jpg".into()],
        display_version: None,
        platforms: vec![],
    }))));
    let next = state.apply(StoreEvent::Image {
        url: urls[0].clone(),
        result: Ok("/c/0.jpg".into()),
    });
    assert!(
        matches!(next.as_slice(), [StoreRequest::Image(url)] if url == "https://x.test/shot.jpg"),
        "{next:?}"
    );
}

// A thumbnail repeated among the screenshots was requested twice while counted once in flight.
#[test]
fn offer_page_art_requests_each_image_once() {
    let mut state = StoreState::new();
    let shot = "https://x.test/shot.jpg".to_owned();
    let sent = state.apply(StoreEvent::Offer(Ok(Box::new(StoreOfferDetail {
        offer: offer("a", Some(&shot), None),
        description: None,
        screenshot_urls: vec![shot.clone(), shot.clone()],
        display_version: None,
        platforms: vec![],
    }))));
    assert_eq!(sent.len(), 1, "{sent:?}");
}
