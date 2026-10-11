//! The vanilla store screens render the store's snapshots: card text, hero rows and the offer page.

use super::tests::{FixedText, NoTextures, screen_texts};

// Cards drew only their price: the title and creator cells and the row header hid behind unset visibility flags.
#[test]
fn store_cards_draw_their_offer_titles() {
    use protocol::store_control::{StoreOffer, StorePrice};
    let offer = |id: &str, title: &str| StoreOffer {
        id: id.into(),
        title: title.into(),
        creator: Some("Studio".into()),
        content_type: None,
        thumbnail_url: None,
        store_id: None,
        prices: vec![StorePrice {
            currency: "mc".into(),
            amount: 830,
        }],
        rating: None,
        tags: vec![],
        owned: false,
    };
    let mut view = launcher::menu::MenuView::new(true, "Player".to_owned());
    view.screen = launcher::menu::MenuScreen::Store;
    view.store = Some(std::sync::Arc::new(launcher::store::StoreSnapshot {
        loading: false,
        rows: vec![launcher::store::DisplayRow {
            id: None,
            title: "New".into(),
            role: "StoreRow",
            offers: vec![offer("a", "Castle Pack"), offer("b", "Pale Garden")],
            continuation: None,
        }],
        ..launcher::store::StoreSnapshot::empty()
    }));
    let Some(texts) = screen_texts(&view) else {
        return;
    };
    assert!(
        texts.iter().any(|t| t.contains("830")),
        "price drawn: {texts:?}"
    );
    for wanted in ["Castle Pack", "Studio", "New"] {
        assert!(
            texts.iter().any(|t| t == wanted),
            "{wanted} drawn: {texts:?}"
        );
    }
}

// The offer page reads title, creator and description as globals; on its row item they never drew.
#[test]
fn store_offer_page_draws_its_title() {
    use protocol::store_control::{StoreOffer, StoreOfferDetail, StorePrice};
    let offer = StoreOffer {
        id: "a".into(),
        title: "Castle Pack".into(),
        creator: Some("Studio".into()),
        content_type: None,
        thumbnail_url: None,
        store_id: None,
        prices: vec![StorePrice {
            currency: "mc".into(),
            amount: 830,
        }],
        rating: None,
        tags: vec![],
        owned: false,
    };
    let mut view = launcher::menu::MenuView::new(true, "Player".to_owned());
    view.screen = launcher::menu::MenuScreen::Store;
    view.store = Some(std::sync::Arc::new(launcher::store::StoreSnapshot {
        loading: false,
        view: launcher::store::StoreView::Detail,
        detail: Some(StoreOfferDetail {
            offer,
            description: Some("A castle.".into()),
            screenshot_urls: vec![],
            display_version: None,
            platforms: vec![],
        }),
        ..launcher::store::StoreSnapshot::empty()
    }));
    let Some(texts) = screen_texts(&view) else {
        return;
    };
    assert!(texts.iter().any(|t| t == "Castle Pack"), "{texts:?}");
    assert!(texts.iter().any(|t| t.contains("A castle.")), "{texts:?}");
}

// A hero row read the page-wide hero collection, which was never filled, so it drew no offers.
#[test]
fn a_hero_row_draws_and_opens_its_offers() {
    use protocol::store_control::StoreOffer;
    let offer = |id: &str| StoreOffer {
        id: id.into(),
        title: id.into(),
        creator: None,
        content_type: None,
        thumbnail_url: Some(format!("https://x.test/{id}.jpg")),
        store_id: None,
        prices: vec![],
        rating: None,
        tags: vec![],
        owned: false,
    };
    let mut view = launcher::menu::MenuView::new(true, "Player".to_owned());
    view.screen = launcher::menu::MenuScreen::Store;
    view.store = Some(std::sync::Arc::new(launcher::store::StoreSnapshot {
        loading: false,
        rows: vec![launcher::store::DisplayRow {
            id: None,
            title: String::new(),
            role: "HeroRow",
            offers: vec![offer("a"), offer("b")],
            continuation: None,
        }],
        images: ["a", "b"]
            .map(|id| (format!("https://x.test/{id}.jpg"), format!("/c/{id}.jpg")))
            .into_iter()
            .collect(),
        ..launcher::store::StoreSnapshot::empty()
    }));
    let Some(carrier) = super::pack_harness::carrier() else {
        return;
    };
    let files = carrier.ui_files();
    let catalog =
        json_ui::Catalog::from_files(files.iter().map(|f| (&*f.path, &*f.bytes))).unwrap();
    let screen = super::menu_screens::screen_data(&view, &|_| None).unwrap();
    let env = json_ui::LayoutEnv {
        text: &FixedText,
        textures: &NoTextures,
    };
    let render = json_ui::render_screen(
        screen.reference,
        &catalog,
        &screen.context,
        &screen.data,
        [480.0, 270.0],
        &env,
        &json_ui::ViewState::default(),
    )
    .unwrap();
    let art: Vec<&str> = render
        .nodes
        .iter()
        .filter_map(|node| match &node.draw {
            json_ui::Draw::Sprite { texture, .. } if texture.starts_with("/c/") => {
                Some(texture.as_str())
            }
            _ => None,
        })
        .collect();
    assert!(
        art.contains(&"/c/a.jpg") && art.contains(&"/c/b.jpg"),
        "{art:?}"
    );
    let snapshot = view.store.as_deref();
    let opens: Vec<_> = render
        .hits
        .iter()
        .filter_map(|region| crate::store::action(snapshot, region))
        .collect();
    assert!(
        opens.contains(&launcher::store::StoreAction::OpenOffer { row: 0, index: 1 }),
        "{opens:?}"
    );
}

// Every hero row read one shared collection, so each drew the last hero row's offers.
#[test]
fn each_hero_row_draws_its_own_offers() {
    use protocol::store_control::StoreOffer;
    let offer = |id: &str| StoreOffer {
        id: id.into(),
        title: id.into(),
        creator: None,
        content_type: None,
        thumbnail_url: Some(format!("https://x.test/{id}.jpg")),
        store_id: None,
        prices: vec![],
        rating: None,
        tags: vec![],
        owned: false,
    };
    let hero = |ids: [&str; 2]| launcher::store::DisplayRow {
        id: None,
        title: String::new(),
        role: "HeroRow",
        offers: ids.map(offer).to_vec(),
        continuation: None,
    };
    let mut view = launcher::menu::MenuView::new(true, "Player".to_owned());
    view.screen = launcher::menu::MenuScreen::Store;
    view.store = Some(std::sync::Arc::new(launcher::store::StoreSnapshot {
        loading: false,
        rows: vec![hero(["a", "b"]), hero(["c", "d"])],
        images: ["a", "b", "c", "d"]
            .map(|id| (format!("https://x.test/{id}.jpg"), format!("/c/{id}.jpg")))
            .into_iter()
            .collect(),
        ..launcher::store::StoreSnapshot::empty()
    }));
    let Some(carrier) = super::pack_harness::carrier() else {
        return;
    };
    let files = carrier.ui_files();
    let catalog =
        json_ui::Catalog::from_files(files.iter().map(|f| (&*f.path, &*f.bytes))).unwrap();
    let screen = super::menu_screens::screen_data(&view, &|_| None).unwrap();
    let env = json_ui::LayoutEnv {
        text: &FixedText,
        textures: &NoTextures,
    };
    let render = json_ui::render_screen(
        screen.reference,
        &catalog,
        &screen.context,
        &screen.data,
        [480.0, 1200.0],
        &env,
        &json_ui::ViewState::default(),
    )
    .unwrap();
    let art: Vec<&str> = render
        .nodes
        .iter()
        .filter_map(|node| match &node.draw {
            json_ui::Draw::Sprite { texture, .. } if texture.starts_with("/c/") => {
                Some(texture.as_str())
            }
            _ => None,
        })
        .collect();
    for id in ["/c/a.jpg", "/c/b.jpg", "/c/c.jpg", "/c/d.jpg"] {
        assert!(art.contains(&id), "{id} missing from {art:?}");
    }
}

/// The vanilla store home for `rows`, rendered with fixed text metrics; `None` without the carrier.
fn store_render(
    rows: Vec<launcher::store::DisplayRow>,
    root: [f64; 2],
) -> Option<json_ui::ScreenRender> {
    let carrier = super::pack_harness::carrier()?;
    let mut view = launcher::menu::MenuView::new(true, "Player".to_owned());
    view.screen = launcher::menu::MenuScreen::Store;
    let images = rows
        .iter()
        .flat_map(|row| &row.offers)
        .filter_map(|offer| offer.thumbnail_url.clone())
        .map(|url| {
            let file = format!("/c/{}", url.rsplit('/').next().unwrap_or_default());
            (url, file)
        })
        .collect();
    view.store = Some(std::sync::Arc::new(launcher::store::StoreSnapshot {
        loading: false,
        rows,
        images,
        ..launcher::store::StoreSnapshot::empty()
    }));
    let files = carrier.ui_files();
    let catalog =
        json_ui::Catalog::from_files(files.iter().map(|f| (&*f.path, &*f.bytes))).unwrap();
    let screen = super::menu_screens::screen_data(&view, &|_| None).unwrap();
    let env = json_ui::LayoutEnv {
        text: &FixedText,
        textures: &NoTextures,
    };
    json_ui::render_screen(
        screen.reference,
        &catalog,
        &screen.context,
        &screen.data,
        root,
        &env,
        &json_ui::ViewState::default(),
    )
}

fn priced_offer(id: &str) -> protocol::store_control::StoreOffer {
    protocol::store_control::StoreOffer {
        id: id.into(),
        title: "Castle".into(),
        creator: Some("Studio".into()),
        content_type: None,
        thumbnail_url: Some(format!("https://x.test/{id}.jpg")),
        store_id: None,
        prices: vec![protocol::store_control::StorePrice {
            currency: "mc".into(),
            amount: 830,
        }],
        rating: Some(protocol::store_control::StoreRating {
            average: 4.4,
            count: 120,
        }),
        tags: vec![],
        owned: false,
    }
}

// Cards drew the content-card info row, which needs service card styles the store sends none of: the
// rating ran into the price, with no star or coin icon.
#[test]
fn store_cards_lay_out_rating_and_price_apart_with_their_icons() {
    let row = launcher::store::DisplayRow {
        id: None,
        title: "New".into(),
        role: "StoreRow",
        offers: vec![priced_offer("a"), priced_offer("b")],
        continuation: None,
    };
    let Some(render) = store_render(vec![row], [640.0, 360.0]) else {
        return;
    };
    let card = render
        .hits
        .iter()
        .find(|region| region.pressed.as_deref() == Some("button.select_offer"))
        .expect("an offer card")
        .rect;
    // Horizontally on the card; labels may overhang its bottom edge by their descent.
    let inside = |rect: &json_ui::RectOut| {
        rect.x >= card.x - 0.5
            && rect.x + rect.w <= card.x + card.w + 0.5
            && rect.y >= card.y
            && rect.y < card.y + card.h
    };
    let text = |wanted: &str| {
        render
            .nodes
            .iter()
            .find(|n| {
                matches!(&n.draw, json_ui::Draw::Text { text, .. } if text == wanted)
                    && inside(&n.dest)
            })
            .map(|n| n.dest)
            .unwrap_or_else(|| panic!("{wanted} not drawn on the card"))
    };
    let (price, rating) = (text("830"), text("4.4"));
    let apart = price.x + price.w <= rating.x || rating.x + rating.w <= price.x;
    assert!(apart, "price {price:?} overlaps rating {rating:?}");
    for icon in ["textures/ui/ratings_fullstar", "/c/a.jpg"] {
        assert!(
            render.nodes.iter().any(
                |n| matches!(&n.draw, json_ui::Draw::Sprite { texture, .. } if texture == icon)
                    && inside(&n.dest)
            ),
            "{icon} not drawn on the card"
        );
    }
}

// Hero cards drew an empty footer: their title and price sat in content-card rows that never filled.
#[test]
fn hero_cards_draw_their_title_and_price() {
    let row = launcher::store::DisplayRow {
        id: None,
        title: String::new(),
        role: "HeroRow",
        offers: vec![priced_offer("a"), priced_offer("b")],
        continuation: None,
    };
    let Some(render) = store_render(vec![row], [640.0, 360.0]) else {
        return;
    };
    let texts: Vec<&str> = render
        .nodes
        .iter()
        .filter_map(|n| match &n.draw {
            json_ui::Draw::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        texts.iter().filter(|t| **t == "Castle").count() >= 2,
        "{texts:?}"
    );
    assert!(
        texts.iter().filter(|t| **t == "830").count() >= 2,
        "{texts:?}"
    );
}
