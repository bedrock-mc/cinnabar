//! The store's presented state: everything a screen build needs, cloneable across the menu view
//! boundary so the presentation side can bind it with its own translator.

use std::collections::HashMap;

use bridge::{StoreOffer, StoreOfferDetail};

use super::flow::{PurchaseDialog, PurchaseFlow};
use super::worker::StoreError;

/// Most offer images the menu artwork atlas packs at once, in draw order.
pub const MAX_VISIBLE_IMAGES: usize = 60;

/// How an offer image is shown, which sets the size it is decoded at.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoreArt {
    /// An offer card's thumbnail.
    Card,
    /// The offer page's key art and screenshots, and a hero row's feature tile.
    Feature,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoreView {
    Home,
    Search,
    Detail,
    Inventory,
}

/// One titled strip of offers as drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct DisplayRow {
    pub id: Option<String>,
    pub title: String,
    /// The vanilla row factory role (`StoreRow`, `GridList`, ...).
    pub role: &'static str,
    pub offers: Vec<StoreOffer>,
    /// Token that loads this row's next offers, when it has more.
    pub continuation: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StoreSnapshot {
    pub view: StoreView,
    pub rows: Vec<DisplayRow>,
    pub detail: Option<StoreOfferDetail>,
    pub balance: Option<i64>,
    pub loading: bool,
    pub failure: Option<StoreError>,
    pub flow: PurchaseFlow,
    /// Thumbnail URL to the local file the core cached.
    pub images: HashMap<String, String>,
    pub owned_total: usize,
    pub search_term: String,
}

impl StoreSnapshot {
    /// The state before anything has loaded.
    pub fn empty() -> Self {
        Self {
            view: StoreView::Home,
            rows: Vec::new(),
            detail: None,
            balance: None,
            loading: true,
            failure: None,
            flow: PurchaseFlow::Idle,
            images: HashMap::new(),
            owned_total: 0,
            search_term: String::new(),
        }
    }

    /// Whether an overlay (progress or a modal) is up and takes the input.
    pub fn modal_active(&self) -> bool {
        match &self.flow {
            PurchaseFlow::Idle => false,
            PurchaseFlow::Done(dialog) => !matches!(dialog, PurchaseDialog::Success { .. }),
            PurchaseFlow::Confirming { .. } | PurchaseFlow::InProgress { .. } => true,
        }
    }

    /// Local files of the offer images currently on screen, in draw order, bounded by the artwork atlas.
    pub fn image_paths(&self) -> Vec<(String, StoreArt)> {
        let detail = self.detail.iter().flat_map(|detail| {
            detail
                .offer
                .thumbnail_url
                .iter()
                .chain(detail.screenshot_urls.iter())
                .map(|url| (url, StoreArt::Feature))
        });
        // The first hero row's first offer is its half-width feature tile; later hero tiles stay
        // card-sized so a page's art still fits the atlas.
        let feature = self.rows.iter().position(|row| row.role == "HeroRow");
        let rows = self.rows.iter().enumerate().flat_map(move |(at, row)| {
            row.offers
                .iter()
                .enumerate()
                .filter_map(move |(index, offer)| {
                    let art = if Some(at) == feature && index == 0 {
                        StoreArt::Feature
                    } else {
                        StoreArt::Card
                    };
                    offer.thumbnail_url.as_ref().map(|url| (url, art))
                })
        });
        let mut seen: Vec<(String, StoreArt)> = Vec::new();
        for (url, art) in detail.chain(rows) {
            let Some(path) = self.images.get(url) else {
                continue;
            };
            // An image shown both as a card and as feature art decodes at the larger size.
            if let Some((_, kept)) = seen.iter_mut().find(|(seen, _)| seen == path) {
                if art == StoreArt::Feature {
                    *kept = art;
                }
                continue;
            }
            if seen.len() == MAX_VISIBLE_IMAGES {
                break;
            }
            seen.push((path.clone(), art));
        }
        seen
    }
}

/// The vanilla row factory a layout row `kind` (its controlId) draws with; `None` for a row the client
/// has no factory for yet (promo banner, nav buttons, coin bundles, the top-bar layout row).
pub fn role_for(kind: Option<&str>) -> Option<&'static str> {
    Some(match kind.unwrap_or("StoreRow") {
        "StoreRow" => "StoreRow",
        "GridList" => "GridList",
        "VerticalGridList" => "VerticalGridList",
        "HeroRow" => "HeroRow",
        "CarouselRow" => "CarouselRow",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offer(id: &str, url: Option<&str>) -> StoreOffer {
        StoreOffer {
            id: id.into(),
            title: id.into(),
            creator: None,
            content_type: None,
            thumbnail_url: url.map(str::to_owned),
            store_id: None,
            prices: vec![],
            rating: None,
            tags: vec![],
            owned: false,
        }
    }

    #[test]
    fn image_paths_follow_draw_order_skip_unfetched_and_dedupe() {
        let snapshot = StoreSnapshot {
            view: StoreView::Home,
            rows: vec![DisplayRow {
                id: None,
                title: String::new(),
                role: "StoreRow",
                offers: vec![
                    offer("a", Some("https://x.test/a")),
                    offer("b", Some("https://x.test/missing")),
                    offer("c", Some("https://x.test/a")),
                    offer("d", Some("https://x.test/d")),
                ],
                continuation: None,
            }],
            detail: None,
            balance: None,
            loading: false,
            failure: None,
            flow: PurchaseFlow::Idle,
            images: [
                ("https://x.test/a".to_owned(), "/c/a.png".to_owned()),
                ("https://x.test/d".to_owned(), "/c/d.png".to_owned()),
            ]
            .into_iter()
            .collect(),
            owned_total: 0,
            search_term: String::new(),
        };
        assert_eq!(
            snapshot.image_paths(),
            [
                ("/c/a.png".to_owned(), StoreArt::Card),
                ("/c/d.png".to_owned(), StoreArt::Card)
            ]
        );
    }

    #[test]
    fn rows_without_a_client_factory_have_no_role() {
        assert_eq!(role_for(Some("GridList")), Some("GridList"));
        assert_eq!(role_for(None), Some("StoreRow"));
        for kind in ["PromoBanner", "NavButtonRow", "CoinBundleRow", "Layout"] {
            assert_eq!(role_for(Some(kind)), None, "{kind}");
        }
    }

    // The hero row's half-width tile was decoded at card size and drew blurred.
    #[test]
    fn a_hero_rows_feature_tile_is_decoded_as_feature_art() {
        let snapshot = StoreSnapshot {
            rows: vec![DisplayRow {
                id: None,
                title: String::new(),
                role: "HeroRow",
                offers: vec![
                    offer("a", Some("https://x.test/a")),
                    offer("b", Some("https://x.test/b")),
                ],
                continuation: None,
            }],
            images: [
                ("https://x.test/a", "/c/a.png"),
                ("https://x.test/b", "/c/b.png"),
            ]
            .map(|(url, path)| (url.to_owned(), path.to_owned()))
            .into_iter()
            .collect(),
            ..StoreSnapshot::empty()
        };
        assert_eq!(
            snapshot.image_paths(),
            [
                ("/c/a.png".to_owned(), StoreArt::Feature),
                ("/c/b.png".to_owned(), StoreArt::Card)
            ]
        );
    }

    // A hero tile sharing its image with an earlier card kept the card size and drew blurred.
    #[test]
    fn a_shared_image_takes_its_largest_art_size() {
        let snapshot = StoreSnapshot {
            rows: vec![
                DisplayRow {
                    id: None,
                    title: String::new(),
                    role: "StoreRow",
                    offers: vec![offer("a", Some("https://x.test/a"))],
                    continuation: None,
                },
                DisplayRow {
                    id: None,
                    title: String::new(),
                    role: "HeroRow",
                    offers: vec![offer("a", Some("https://x.test/a"))],
                    continuation: None,
                },
            ],
            images: [("https://x.test/a".to_owned(), "/c/a.png".to_owned())]
                .into_iter()
                .collect(),
            ..StoreSnapshot::empty()
        };
        assert_eq!(
            snapshot.image_paths(),
            [("/c/a.png".to_owned(), StoreArt::Feature)]
        );
    }
}
