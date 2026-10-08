//! Offer-card values for the vanilla store layouts: the binding names `common_store` and
//! `store_item_list` read from a card, its info rows and its info columns.

use std::collections::BTreeMap;

use json_ui::{CollectionItem, Scalar};
use protocol::store_control::StoreOffer;

/// The vanilla `texture_file_system` value for a path outside the packs.
pub const RAW_PATH: &str = "RawPath";

pub type Translate<'a> = &'a dyn Fn(&str) -> String;

pub fn text(value: impl Into<String>) -> Scalar {
    Scalar::Text(value.into())
}

/// "You Have %s Minecoins" is the wallet tooltip; the header shows the bare amount.
#[cfg(test)]
pub(super) fn balance_text(amount: i64, tr: Translate<'_>) -> String {
    tr("store.coins.currentCoins").replace("%s", &amount.to_string())
}

/// The info columns of a standard card, one list per row (title, creator, then the price line).
const INFO_ROWS: &[&[&str]] = &[
    &["title"],
    &["creator"],
    &[
        "iconCoin",
        "currentPrice",
        "originalPrice",
        "fill",
        "ratings",
    ],
];

/// The card values for one offer. `image_path` maps a thumbnail URL to a decoded local file.
pub fn offer_values(
    offer: &StoreOffer,
    image_path: &dyn Fn(&str) -> Option<String>,
    tr: Translate<'_>,
) -> BTreeMap<String, Scalar> {
    let price = offer.prices.first().filter(|price| price.amount > 0);
    let payable = price.is_some() && !offer.owned;
    let prompt = if offer.owned {
        tr("store.owned")
    } else if let Some(price) = price {
        price.amount.to_string()
    } else {
        tr("store.free")
    };
    let creator = offer.creator.clone().unwrap_or_default();
    let (rating_text, ratings) = offer.rating.as_ref().map_or((String::new(), 0), |rating| {
        (format!("{:.1}", rating.average), rating.count)
    });
    let path = offer
        .thumbnail_url
        .as_deref()
        .and_then(image_path)
        .unwrap_or_default();
    let file_system = if path.is_empty() { "" } else { RAW_PATH };
    let mut values = BTreeMap::new();
    let mut set = |name: &str, value: Scalar| {
        values.insert(name.to_owned(), value);
    };
    set("#title_label", text(offer.title.clone()));
    // The title and creator info cells bind `#visible` to this name unless a cell names its own.
    set("#offer_info_text_visible_binding", Scalar::Bool(true));
    set(
        "#is_creator_label_visible",
        Scalar::Bool(!creator.is_empty()),
    );
    set("#creator_label", text(creator));
    set("#offer_coin_visible", Scalar::Bool(payable));
    set("#offer_info_price_visibile", Scalar::Bool(payable));
    set("#offer_prompt_text", text(prompt.clone()));
    // The pre-content-card cards show the prompt (price, Owned or Free) beside the coin icon.
    set("#offer_prompt_text_visibility", Scalar::Bool(true));
    set("#offer_full_price", text(prompt));
    set("#offer_strikethrough_price_visible", Scalar::Bool(false));
    set(
        "#offer_minecoin_text",
        text(price.map_or_else(String::new, |p| p.amount.to_string())),
    );
    set("#ratings_visible", Scalar::Bool(ratings > 0));
    set("#rating_text", text(rating_text));
    // The count label packs flush against the average, so it stays empty (and hidden) on cards.
    set("#ratings_count_text", text(""));
    set("#offer_markdown_visible", Scalar::Bool(false));
    set("#new_offer_icon_visible", Scalar::Bool(false));
    set("#progress_visible", Scalar::Bool(false));
    set("#item_does_not_meet_requirements", Scalar::Bool(false));
    set("#rtx_label_visible", Scalar::Bool(false));
    set("#offer_realms_visible", Scalar::Bool(false));
    set("#icon_overlay_position_collection", Scalar::Num(0.0));
    set("#valid_offer_index", Scalar::Bool(true));
    set("#thumbnail_texture_file_system", text(file_system));
    set("#thumbnail_texture_path", text(path.clone()));
    // The pre-content-card hero tiles read the same art as key art.
    set("#key_art_texture_file_system", text(file_system));
    set("#key_art_texture_path", text(path));
    values
}

/// One collection item for `offer`; `role` selects the factory control (e.g. `Generic`).
pub fn offer_item(
    role: &str,
    offer: &StoreOffer,
    image_path: &dyn Fn(&str) -> Option<String>,
    tr: Translate<'_>,
) -> CollectionItem {
    CollectionItem {
        role: Some(role.to_owned()),
        values: offer_values(offer, image_path, tr),
    }
}

/// The trailing "See All" item of a row that has more offers.
pub fn show_more_item(tr: Translate<'_>) -> CollectionItem {
    CollectionItem::new("ShowMoreButton").with("#show_more_text", text(tr("store.showMore")))
}

/// The info rows every card lays out, and the columns inside each.
pub fn info_rows() -> (Vec<CollectionItem>, Vec<Vec<CollectionItem>>) {
    let rows = INFO_ROWS
        .iter()
        .map(|columns| {
            CollectionItem::new("row").with(
                "#offer_info_column_collection",
                Scalar::Num(columns.len() as f64),
            )
        })
        .collect();
    let columns = INFO_ROWS
        .iter()
        .map(|columns| {
            columns
                .iter()
                .map(|role| CollectionItem::new(*role))
                .collect()
        })
        .collect();
    (rows, columns)
}

/// The number of info rows, bound once for the whole screen.
pub fn info_row_count() -> f64 {
    INFO_ROWS.len() as f64
}

#[cfg(test)]
mod tests {
    use protocol::store_control::{StorePrice, StoreRating};

    use super::*;

    fn tr(key: &str) -> String {
        match key {
            "store.owned" => "Owned".into(),
            "store.free" => "Free".into(),
            "store.showMore" => "See All".into(),
            "store.coins.currentCoins" => "You Have %s Minecoins".into(),
            other => format!("<{other}>"),
        }
    }

    fn offer(title: &str, price: Option<i64>, owned: bool) -> StoreOffer {
        StoreOffer {
            id: title.into(),
            title: title.into(),
            creator: Some("Studio".into()),
            content_type: None,
            thumbnail_url: Some(format!("https://x.test/{title}.png")),
            store_id: None,
            prices: price
                .map(|amount| {
                    vec![StorePrice {
                        currency: "mc".into(),
                        amount,
                    }]
                })
                .unwrap_or_default(),
            rating: Some(StoreRating {
                average: 4.5,
                count: 12,
            }),
            tags: vec![],
            owned,
        }
    }

    fn value<'a>(values: &'a BTreeMap<String, Scalar>, name: &str) -> &'a Scalar {
        values.get(name).unwrap_or_else(|| panic!("missing {name}"))
    }

    #[test]
    fn priced_owned_and_free_cards_bind_the_price_line() {
        let path = |url: &str| {
            url.ends_with("paid.png")
                .then(|| "/cache/paid.png".to_owned())
        };
        let paid = offer_values(&offer("paid", Some(320), false), &path, &tr);
        assert_eq!(value(&paid, "#offer_prompt_text"), &text("320"));
        assert_eq!(
            value(&paid, "#offer_info_price_visibile"),
            &Scalar::Bool(true)
        );
        assert_eq!(value(&paid, "#offer_coin_visible"), &Scalar::Bool(true));
        assert_eq!(
            value(&paid, "#thumbnail_texture_path"),
            &text("/cache/paid.png")
        );
        assert_eq!(
            value(&paid, "#thumbnail_texture_file_system"),
            &text("RawPath")
        );
        assert_eq!(value(&paid, "#rating_text"), &text("4.5"));
        let mine = offer_values(&offer("mine", Some(320), true), &path, &tr);
        assert_eq!(value(&mine, "#offer_prompt_text"), &text("Owned"));
        assert_eq!(
            value(&mine, "#offer_info_price_visibile"),
            &Scalar::Bool(false)
        );
        let gift = offer_values(&offer("gift", None, false), &path, &tr);
        assert_eq!(value(&gift, "#offer_prompt_text"), &text("Free"));
        assert_eq!(value(&gift, "#thumbnail_texture_file_system"), &text(""));
    }

    #[test]
    fn info_rows_carry_their_column_counts() {
        let (rows, columns) = info_rows();
        assert_eq!(rows.len(), columns.len());
        assert_eq!(rows.len() as f64, info_row_count());
        assert_eq!(
            rows[2].values.get("#offer_info_column_collection"),
            Some(&Scalar::Num(5.0))
        );
        assert_eq!(columns[2].len(), 5);
        assert_eq!(columns[2][1].role.as_deref(), Some("currentPrice"));
    }

    #[test]
    fn balance_and_show_more_use_vanilla_keys() {
        assert_eq!(balance_text(1500, &tr), "You Have 1500 Minecoins");
        assert_eq!(show_more_item(&tr).role.as_deref(), Some("ShowMoreButton"));
    }
}
