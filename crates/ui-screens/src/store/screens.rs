//! The per-screen binding sets the vanilla store controllers feed: each view of a snapshot becomes
//! one engine screen (plus at most one overlay) with its globals and collections.
//!
//! Deliberately unbound, on every store screen: text colours, fonts and offsets of the offer info
//! (they keep the layout's own values), badges and icon overlays, the tag/genre/language buttons,
//! wishlist, share, video, ratings submission, filters and sorting, the sidebar navigation and the
//! search box's text entry.

use std::collections::BTreeMap;

use json_ui::{
    CollectionItem, Context, DataSource, FormModel, ModalForm, Scalar, form_context,
    form_data_source, form_template, scoped_key,
};

#[cfg(test)]
use super::bindings::balance_text;
use super::bindings::{
    RAW_PATH, Translate, info_row_count, info_rows, offer_item, show_more_item, text,
};
use super::flow::PurchaseFlowPresentation;
use launcher::store::snapshot::{DisplayRow, StoreSnapshot, StoreView};
use launcher::store::worker::StoreError;

use launcher::store::SDL_SCREEN;
pub const INVENTORY_SCREEN: &str = "store_inventory.store_inventory_screen";
pub const PROGRESS_SCREEN: &str = "store_progress.store_progress_screen";

/// The name of the collection holding the header bar item and every row.
const FACTORY: &str = "factory_collection";
const OFFERS: &str = "offer_collection";
const GRID_FACTORY: &str = "offer_grid_factory";
const HERO_COLLECTION: &str = "hero_row_collection";
const INFO_ROWS_NAME: &str = "offer_info_row_factory";
const INFO_COLUMNS_NAME: &str = "offer_info_column_factory";

/// Plain screen data, inspectable before it becomes an engine [`DataSource`].
#[derive(Clone, Debug, Default)]
pub struct ScreenData {
    pub globals: BTreeMap<String, Scalar>,
    pub collections: BTreeMap<String, Vec<CollectionItem>>,
    /// `(parent key, index, name)` -> items, for lists that belong to one enclosing item.
    pub scoped: Vec<(String, usize, String, Vec<CollectionItem>)>,
}

impl ScreenData {
    fn global(&mut self, name: &str, value: Scalar) {
        self.globals.insert(name.to_owned(), value);
    }

    fn flag(&mut self, name: &str, on: bool) {
        self.global(name, Scalar::Bool(on));
    }

    fn flags(&mut self, names: &[&str], on: bool) {
        for name in names {
            self.flag(name, on);
        }
    }

    fn into_source(self) -> DataSource {
        let mut data = DataSource::new();
        data.set_strict(true);
        for (name, value) in self.globals {
            data.set_global(name, value);
        }
        for (name, items) in self.collections {
            data.set_collection(name, items);
        }
        for (parent, index, name, items) in self.scoped {
            data.set_scoped_collection(&parent, index, &name, items);
        }
        data
    }
}

/// One engine screen to draw.
pub struct ScreenSpec {
    pub reference: &'static str,
    pub context: Context,
    pub data: DataSource,
}

/// The screen for a snapshot plus the overlay (progress or modal) on top of it, if any.
pub struct StoreScreens {
    pub base: ScreenSpec,
    pub overlay: Option<ScreenSpec>,
}

fn sdl_context(base: &Context) -> Context {
    base.clone()
        .with_flag("content_cards_enabled", false)
        .with_flag("is_sidebar_navigation_enabled", false)
        .with_flag("use_animation", false)
}

/// Build the screens for `snapshot`; `tr` maps a vanilla lang key to text and `base` is the menu's
/// platform context.
pub fn screens(snapshot: &StoreSnapshot, base: &Context, tr: Translate<'_>) -> StoreScreens {
    let (reference, context, data) = match snapshot.view {
        StoreView::Inventory => (
            INVENTORY_SCREEN,
            sdl_context(base),
            inventory_data(snapshot, tr),
        ),
        _ => (SDL_SCREEN, sdl_context(base), layout_data(snapshot, tr)),
    };
    StoreScreens {
        base: ScreenSpec {
            reference,
            context,
            data: data.into_source(),
        },
        overlay: overlay(snapshot, base, tr),
    }
}

fn image_lookup(snapshot: &StoreSnapshot) -> impl Fn(&str) -> Option<String> + '_ {
    |url| snapshot.images.get(url).cloned()
}

/// The overlay the purchase flow wants: the progress screen while running, else a modal.
fn overlay(snapshot: &StoreSnapshot, base: &Context, tr: Translate<'_>) -> Option<ScreenSpec> {
    use launcher::store::flow::PurchaseFlow;
    if matches!(snapshot.flow, PurchaseFlow::InProgress { .. }) {
        let mut data = ScreenData::default();
        data.global(
            "#tooltip_text",
            text(tr("store.popup.purchaseInProgress.msg")),
        );
        return Some(ScreenSpec {
            reference: PROGRESS_SCREEN,
            context: base.clone(),
            data: data.into_source(),
        });
    }
    let modal = snapshot.flow.modal(tr)?;
    Some(modal_screen(&modal, base))
}

/// A vanilla popup for `modal`; a one-button modal uses the single-button layout.
pub fn modal_screen(modal: &ModalForm, base: &Context) -> ScreenSpec {
    let model = FormModel::Modal(modal.clone());
    let single = modal.button2.is_empty();
    let context = form_context(&model, base)
        .with_flag("two_buttons_visible", !single)
        .with_flag("single_button_visible", single);
    let mut data = form_data_source(&model);
    if single {
        data.set_global(
            "#modal_middle_button_text",
            Scalar::Text(modal.button1.clone()),
        );
        data.set_global("#modal_left_button_text", Scalar::Text(String::new()));
    }
    ScreenSpec {
        reference: form_template(&model),
        context,
        data,
    }
}

fn header(data: &mut ScreenData, snapshot: &StoreSnapshot, tr: Translate<'_>) {
    data.global("#screen_header_title", text(tr("store.title")));
    data.global(
        "#coin_balance",
        text(
            snapshot
                .balance
                .map_or_else(String::new, |amount| amount.to_string()),
        ),
    );
    data.global("#coin_purchase_in_progress", Scalar::Bool(false));
    data.global("#has_navigation", Scalar::Bool(false));
    data.global("#sdl_top_minecoin_content", Scalar::Num(1.0));
    data.flag("#inventory_button_visible", true);
    data.flag("#show_clear_text_button", !snapshot.search_term.is_empty());
    data.flags(
        &[
            "#gamepad_helper_visible",
            "#is_top_row_button_focus_enabled",
            "#newline_refresh",
        ],
        false,
    );
    data.global("#page_loading_visible", Scalar::Bool(snapshot.loading));
    data.global("#progress_visible", Scalar::Bool(snapshot.loading));
    failure(data, snapshot.failure, tr);
    data.global("#offer_info_row_collection", Scalar::Num(info_row_count()));
}

fn failure(data: &mut ScreenData, failure: Option<StoreError>, tr: Translate<'_>) {
    data.flag("#store_error_visible", failure.is_some());
    data.global("#store_failure_code", text(""));
    let message = match failure {
        None => String::new(),
        Some(StoreError::SignedOut) => tr("store.popup.xblRequired.message"),
        Some(_) => tr("store.connection.failed.body"),
    };
    data.global("#store_failure_text", text(message));
}

fn top_bar() -> CollectionItem {
    CollectionItem::new("TopBar")
}

/// Home and search rows, or the offer page's sections, as the `factory_collection`.
fn layout_data(snapshot: &StoreSnapshot, tr: Translate<'_>) -> ScreenData {
    let mut data = ScreenData::default();
    header(&mut data, snapshot, tr);
    let mut items = vec![top_bar()];
    match snapshot.view {
        StoreView::Detail => detail_sections(&mut data, &mut items, snapshot, tr),
        _ => {
            if snapshot.view == StoreView::Search {
                items.push(search_bar_item(snapshot));
                data.flags(
                    &[
                        "#search_bar_enabled",
                        "#is_search_offer_list_visible",
                        "#search_results_panel_visible",
                    ],
                    true,
                );
                data.flag("#search_spinner_visible", snapshot.loading);
                data.flag(
                    "#search_error_panel_visible",
                    !snapshot.loading && snapshot.rows.iter().all(|row| row.offers.is_empty()),
                );
                data.global("#tts_filters_appliedCount_text", text(""));
                let more = snapshot.rows.iter().any(|row| row.continuation.is_some());
                data.flag("#pagination_visible", more);
                data.flag("#next_enabled", more);
                data.global("#page_number_text", text(""));
            }
            for row in &snapshot.rows {
                let index = items.len();
                items.push(row_item(row));
                offer_lists(&mut data, index, row, snapshot, tr);
            }
        }
    }
    data.global("#store_section_content", Scalar::Num(items.len() as f64));
    data.collections.insert(FACTORY.to_owned(), items);
    data
}

fn search_bar_item(snapshot: &StoreSnapshot) -> CollectionItem {
    CollectionItem::new("SearchBar")
        .with("#item_name", text(snapshot.search_term.clone()))
        .with("#enabled", Scalar::Bool(false))
}

fn row_item(row: &DisplayRow) -> CollectionItem {
    let grid = matches!(row.role, "GridList" | "VerticalGridList");
    let count = row.offers.len() + usize::from(row.continuation.is_some() && !grid);
    CollectionItem::new(row.role)
        .with("#offer_collection_visible", Scalar::Bool(true))
        .with("#offer_collection_ready", Scalar::Bool(true))
        .with("#sdl_dropdown_data_row_visible", Scalar::Bool(true))
        .with(
            "#section_title_visible",
            Scalar::Bool(!row.title.is_empty()),
        )
        .with("#section_header", text(row.title.clone()))
        .with("#header_text_color", text("#ffffff"))
        .with("#show_header_background", Scalar::Bool(false))
        .with("#show_banner", Scalar::Bool(false))
        // The plain row header shows while the sales banner header is hidden.
        .with("#hide_banner", Scalar::Bool(true))
        .with("#show_timer", Scalar::Bool(false))
        .with("#show_row_background", Scalar::Bool(false))
        .with("#show_row_outline", Scalar::Bool(false))
        .with("#store_offer_row_content", Scalar::Num(count as f64))
        .with("#offer_grid_type", Scalar::Num(1.0))
        .with("#indent", Scalar::Num(0.0))
}

/// The offers of one row (and the "See All" tail), each with its info rows and columns, scoped to
/// the row's item in the factory collection.
fn offer_lists(
    data: &mut ScreenData,
    factory_index: usize,
    row: &DisplayRow,
    snapshot: &StoreSnapshot,
    tr: Translate<'_>,
) {
    let path = image_lookup(snapshot);
    let is_grid = matches!(row.role, "GridList" | "VerticalGridList");
    // Without content card styles a row draws vanilla's pre-content-card offer panel.
    let role = if is_grid { "Generic" } else { "GenericOLD" };
    let mut items: Vec<CollectionItem> = row
        .offers
        .iter()
        .map(|offer| offer_item(role, offer, &path, tr))
        .collect();
    if row.continuation.is_some() && !is_grid {
        items.push(show_more_item(tr));
    }
    if row.role == "HeroRow" {
        // A hero row reads `hero_row_collection`, scoped to its own factory item.
        let hero = row
            .offers
            .iter()
            .map(|offer| offer_item("Generic", offer, &path, tr));
        data.scoped.push((
            FACTORY.to_owned(),
            factory_index,
            HERO_COLLECTION.to_owned(),
            hero.collect(),
        ));
    }
    if is_grid {
        // A grid row is one `Generic` grid item that owns the offer list.
        let grid_key = scoped_key(FACTORY, factory_index, GRID_FACTORY);
        data.scoped.push((
            FACTORY.to_owned(),
            factory_index,
            GRID_FACTORY.to_owned(),
            vec![CollectionItem::new("Generic")],
        ));
        info_lists(data, &scoped_key(&grid_key, 0, OFFERS), row.offers.len());
        data.scoped.push((grid_key, 0, OFFERS.to_owned(), items));
    } else {
        info_lists(
            data,
            &scoped_key(FACTORY, factory_index, OFFERS),
            row.offers.len(),
        );
        data.scoped
            .push((FACTORY.to_owned(), factory_index, OFFERS.to_owned(), items));
    }
}

/// The info rows/columns of the first `offers` cards of the list stored at `list_key`.
fn info_lists(data: &mut ScreenData, list_key: &str, offers: usize) {
    for offer in 0..offers {
        let (rows, columns) = info_rows();
        let rows_key = scoped_key(list_key, offer, INFO_ROWS_NAME);
        for (row, columns) in columns.into_iter().enumerate() {
            data.scoped
                .push((rows_key.clone(), row, INFO_COLUMNS_NAME.to_owned(), columns));
        }
        data.scoped
            .push((list_key.to_owned(), offer, INFO_ROWS_NAME.to_owned(), rows));
    }
}

/// The offer page: summary with price and purchase button, screenshots, description.
fn detail_sections(
    data: &mut ScreenData,
    items: &mut Vec<CollectionItem>,
    snapshot: &StoreSnapshot,
    tr: Translate<'_>,
) {
    let Some(detail) = &snapshot.detail else {
        return;
    };
    let offer = &detail.offer;
    let path = image_lookup(snapshot);
    let price = offer.prices.iter().find(|price| price.amount > 0);
    let payable = price.is_some() && !offer.owned;
    let affordable = match (price, snapshot.balance) {
        (Some(price), Some(balance)) => balance >= price.amount,
        _ => false,
    };
    let price_text = price.map_or_else(String::new, |price| price.amount.to_string());
    let key_art = offer
        .thumbnail_url
        .as_deref()
        .and_then(&path)
        .unwrap_or_default();
    data.flag("#purchase_panel_visible", payable);
    data.flag("#activated_purchase_panel_visible", payable && affordable);
    data.flag(
        "#deactivated_purchase_panel_visible",
        payable && !affordable,
    );
    data.flags(
        &["#buttons_panel_visible", "#purchase_buttons_enabled"],
        true,
    );
    data.flag("#coin_visible", payable);
    data.global("#purchase_with_coins_button_text", text(price_text.clone()));
    data.global(
        "#tts_purchase_with_coins_button_text",
        text(price_text.clone()),
    );
    data.global("#full_price", text(price_text.clone()));
    data.global("#tts_full_price", text(price_text));
    data.global(
        "#offer_prompt_text",
        text(if offer.owned {
            tr("store.owned")
        } else {
            String::new()
        }),
    );
    data.flags(
        &[
            "#is_on_sale",
            "#currency_purchase_visible",
            "#action_button_visible",
            "#action_button_enabled",
            "#download_info_visible",
            "#download_progress_bar_visible",
            "#entitlements_refreshing_visible",
            "#exit_world_button_visible",
            "#mpp_free_promo_button_visible",
            "#is_leaving_mpp_banner_visible",
            "#rtx_label_visible",
            "#wishlist_button_visible",
            "#wishlist_button_enabled",
            "#share_button_enabled",
            "#video_button_enabled",
            "#in_csb_button_visible",
            "#nav_grid_visible",
            "#update_check_visible",
            "#update_notification_visible",
            "#show_warning",
        ],
        false,
    );
    data.flag("#progress_loading_anim_visible", snapshot.loading);
    // Gates the whole summary section (title, creator, ratings, key art).
    data.flag("#summary_content_visible", true);
    data.global("#main_mashup_key_art_texture", text(key_art.clone()));
    data.global(
        "#main_mashup_key_art_file_system",
        text(if key_art.is_empty() { "" } else { RAW_PATH }),
    );
    data.global(
        "#creator_nav_grid_visible",
        Scalar::Bool(offer.creator.is_some()),
    );

    // The offer page reads its title, creator and description as globals, not from its row item.
    data.global("#title_label", text(offer.title.clone()));
    data.global(
        "#creator_label",
        text(offer.creator.clone().unwrap_or_default()),
    );
    data.flag("#is_creator_label_visible", offer.creator.is_some());
    items.push(
        CollectionItem::new("ItemSummary")
            .with("#section_title_visible", Scalar::Bool(false))
            .with("#ratings_visible", Scalar::Bool(offer.rating.is_some()))
            .with(
                "#number_of_ratings",
                text(offer.rating.as_ref().map_or(0, |r| r.count).to_string()),
            )
            .with(
                "#rating_footer_text",
                text(offer.rating.as_ref().map_or_else(String::new, |r| {
                    tr("store.ratings.ratingOutOfFive").replace("%s", &format!("{:.1}", r.average))
                })),
            )
            .with("#thumbnail_texture_path", text(key_art.clone()))
            .with(
                "#thumbnail_texture_file_system",
                text(if key_art.is_empty() { "" } else { RAW_PATH }),
            ),
    );
    if !detail.screenshot_urls.is_empty() {
        let shots: Vec<CollectionItem> = detail
            .screenshot_urls
            .iter()
            .map(|url| {
                let shot = snapshot.images.get(url).cloned().unwrap_or_default();
                CollectionItem::default()
                    .with("#screenshot_texture", text(shot.clone()))
                    .with(
                        "#screenshot_texture_file_system",
                        text(if shot.is_empty() { "" } else { RAW_PATH }),
                    )
            })
            .collect();
        let index = items.len();
        items.push(
            CollectionItem::new("ImageGallery")
                .with("#section_title_visible", Scalar::Bool(false))
                .with("#collection_length", Scalar::Num(shots.len() as f64)),
        );
        data.scoped.push((
            FACTORY.to_owned(),
            index,
            "screenshot_collection".to_owned(),
            shots,
        ));
    }
    if let Some(description) = detail.description.as_deref().filter(|d| !d.is_empty()) {
        data.global("#description_label", text(description));
        data.flag("#is_description_expanded", true);
        items.push(
            CollectionItem::new("ItemDescription")
                .with("#section_title_visible", Scalar::Bool(false)),
        );
    }
}

/// "My Library": the owned content that resolved to a catalog entry.
fn inventory_data(snapshot: &StoreSnapshot, tr: Translate<'_>) -> ScreenData {
    let mut data = ScreenData::default();
    header(&mut data, snapshot, tr);
    let path = image_lookup(snapshot);
    let offers: Vec<CollectionItem> = snapshot
        .rows
        .iter()
        .flat_map(|row| row.offers.iter())
        .map(|offer| offer_item("Generic", offer, &path, tr))
        .collect();
    data.global("#inventory_section_content", Scalar::Num(1.0));
    data.global("#max_grid_offers", Scalar::Num(offers.len() as f64));
    data.global("#collection_count", text(snapshot.owned_total.to_string()));
    data.flags(&["#grid_list_visible", "#collections_icon_visible"], true);
    data.flags(
        &[
            "#realms_enabled",
            "#subcategories_visible",
            "#addons_visible",
            "#show_signin_button",
            "#show_no_xbl_and_local_content_warning",
            "#show_no_xbl_and_no_local_content_warning",
            "#category_addons_icon_visible",
            "#category_mashups_icon_visible",
            "#category_skins_icon_visible",
            "#category_textures_icon_visible",
            "#category_worlds_icon_visible",
            "#toggle_on_hover",
        ],
        false,
    );
    for count in [
        "#addons_count",
        "#mashups_count",
        "#skins_count",
        "#textures_count",
        "#worlds_count",
    ] {
        data.global(count, text("0"));
    }
    data.collections.insert(
        "right_pane_factory_collection".to_owned(),
        vec![CollectionItem::new("items_collection_tab")],
    );
    info_lists(&mut data, "items_collection", offers.len());
    data.collections
        .insert("items_collection".to_owned(), offers);
    data
}

#[cfg(test)]
mod tests {
    use bridge::{StoreOffer, StoreOfferDetail, StorePrice};

    use launcher::store::flow::{PurchaseDialog, PurchaseFlow};
    use {
        super::*,
        launcher::store::snapshot::{DisplayRow, StoreSnapshot, StoreView},
        launcher::store::worker::StoreError,
    };

    fn tr(key: &str) -> String {
        format!("<{key}>")
    }

    fn offer(id: &str, price: Option<i64>, owned: bool) -> StoreOffer {
        StoreOffer {
            id: id.into(),
            title: id.into(),
            creator: Some("Studio".into()),
            content_type: None,
            thumbnail_url: Some(format!("https://x.test/{id}.png")),
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
            owned,
        }
    }

    fn snapshot(view: StoreView) -> StoreSnapshot {
        StoreSnapshot {
            view,
            rows: vec![DisplayRow {
                id: None,
                title: "Featured".into(),
                role: "StoreRow",
                offers: vec![offer("a", Some(320), false), offer("b", None, true)],
                continuation: Some("more".into()),
            }],
            detail: None,
            balance: Some(1000),
            loading: false,
            failure: None,
            flow: PurchaseFlow::Idle,
            images: [("https://x.test/a.png".to_owned(), "/c/a.png".to_owned())]
                .into_iter()
                .collect(),
            owned_total: 1,
            search_term: String::new(),
        }
    }

    fn build(view: StoreView) -> ScreenData {
        let snap = snapshot(view);
        match view {
            StoreView::Inventory => inventory_data(&snap, &tr),
            _ => layout_data(&snap, &tr),
        }
    }

    fn scoped<'a>(
        data: &'a ScreenData,
        parent: &str,
        index: usize,
        name: &str,
    ) -> &'a Vec<CollectionItem> {
        data.scoped
            .iter()
            .find(|(p, i, n, _)| p == parent && *i == index && n == name)
            .map(|(_, _, _, items)| items)
            .unwrap_or_else(|| panic!("missing {parent}[{index}].{name}"))
    }

    #[test]
    fn home_lists_the_header_item_then_rows_with_their_offers_and_show_more() {
        let data = build(StoreView::Home);
        let factory = &data.collections[FACTORY];
        assert_eq!(factory[0].role.as_deref(), Some("TopBar"));
        assert_eq!(factory[1].role.as_deref(), Some("StoreRow"));
        assert_eq!(data.globals["#store_section_content"], Scalar::Num(2.0));
        assert_eq!(data.globals["#coin_balance"], text("1000"));
        assert_eq!(factory[1].values["#section_header"], text("Featured"));
        assert_eq!(
            factory[1].values["#store_offer_row_content"],
            Scalar::Num(3.0)
        );
        let offers = scoped(&data, FACTORY, 1, OFFERS);
        assert_eq!(offers.len(), 3);
        assert_eq!(offers[2].role.as_deref(), Some("ShowMoreButton"));
        assert_eq!(
            offers[0].values["#thumbnail_texture_path"],
            text("/c/a.png")
        );
    }

    #[test]
    fn a_grid_row_nests_its_offers_under_one_grid_item() {
        let mut snap = snapshot(StoreView::Home);
        snap.rows[0].role = "GridList";
        let data = layout_data(&snap, &tr);
        assert_eq!(scoped(&data, FACTORY, 1, GRID_FACTORY).len(), 1);
        let grid_key = scoped_key(FACTORY, 1, GRID_FACTORY);
        assert_eq!(
            scoped(&data, &grid_key, 0, OFFERS).len(),
            2,
            "a grid pages by scrolling, not by a trailing button"
        );
        let list = scoped_key(&grid_key, 0, OFFERS);
        assert_eq!(scoped(&data, &list, 0, INFO_ROWS_NAME).len(), 3);
    }

    #[test]
    fn every_card_gets_its_own_info_rows_and_columns() {
        let data = build(StoreView::Home);
        let list = scoped_key(FACTORY, 1, OFFERS);
        for card in 0..2 {
            let rows = scoped(&data, &list, card, INFO_ROWS_NAME);
            assert_eq!(rows.len(), 3);
            let rows_key = scoped_key(&list, card, INFO_ROWS_NAME);
            assert_eq!(scoped(&data, &rows_key, 2, INFO_COLUMNS_NAME).len(), 5);
        }
        assert_eq!(data.globals["#offer_info_row_collection"], Scalar::Num(3.0));
    }

    #[test]
    fn search_adds_the_search_bar_and_empty_state_flags() {
        let mut snap = snapshot(StoreView::Search);
        snap.rows[0].offers.clear();
        let data = layout_data(&snap, &tr);
        assert_eq!(
            data.collections[FACTORY][1].role.as_deref(),
            Some("SearchBar")
        );
        assert_eq!(
            data.globals["#search_error_panel_visible"],
            Scalar::Bool(true)
        );
        assert_eq!(data.globals["#search_bar_enabled"], Scalar::Bool(true));
    }

    #[test]
    fn the_offer_page_binds_price_owned_state_screenshots_and_description() {
        let mut snap = snapshot(StoreView::Detail);
        snap.detail = Some(StoreOfferDetail {
            offer: offer("a", Some(320), false),
            description: Some("A castle".into()),
            screenshot_urls: vec!["https://x.test/s1.png".into()],
            display_version: None,
            platforms: vec![],
        });
        snap.images
            .insert("https://x.test/s1.png".into(), "/c/s1.png".into());
        let data = layout_data(&snap, &tr);
        let roles: Vec<_> = data.collections[FACTORY]
            .iter()
            .map(|i| i.role.as_deref().unwrap())
            .collect();
        assert_eq!(
            roles,
            ["TopBar", "ItemSummary", "ImageGallery", "ItemDescription"]
        );
        assert_eq!(data.globals["#purchase_panel_visible"], Scalar::Bool(true));
        assert_eq!(
            data.globals["#activated_purchase_panel_visible"],
            Scalar::Bool(true)
        );
        assert_eq!(
            data.globals["#purchase_with_coins_button_text"],
            text("320")
        );
        assert_eq!(
            scoped(&data, FACTORY, 2, "screenshot_collection")[0].values["#screenshot_texture"],
            text("/c/s1.png")
        );
        snap.balance = Some(10);
        let poor = layout_data(&snap, &tr);
        assert_eq!(
            poor.globals["#deactivated_purchase_panel_visible"],
            Scalar::Bool(true)
        );
        snap.detail.as_mut().unwrap().offer.owned = true;
        let owned = layout_data(&snap, &tr);
        assert_eq!(
            owned.globals["#purchase_panel_visible"],
            Scalar::Bool(false)
        );
        assert_eq!(owned.globals["#offer_prompt_text"], text("<store.owned>"));
    }

    #[test]
    fn the_library_lists_owned_offers_in_the_items_collection() {
        let data = build(StoreView::Inventory);
        assert_eq!(data.collections["items_collection"].len(), 2);
        assert_eq!(
            data.collections["right_pane_factory_collection"][0]
                .role
                .as_deref(),
            Some("items_collection_tab")
        );
        assert_eq!(data.globals["#collection_count"], text("1"));
        assert_eq!(data.globals["#realms_enabled"], Scalar::Bool(false));
    }

    #[test]
    fn progress_and_modal_overlays_follow_the_purchase_flow() {
        let base = Context::desktop();
        let mut snap = snapshot(StoreView::Detail);
        assert!(overlay(&snap, &base, &tr).is_none());
        snap.flow = PurchaseFlow::InProgress {
            purchase_id: "id".into(),
            offer_title: "a".into(),
        };
        assert_eq!(
            overlay(&snap, &base, &tr).map(|s| s.reference),
            Some(PROGRESS_SCREEN)
        );
        snap.flow = PurchaseFlow::Done(PurchaseDialog::Disabled);
        assert_eq!(
            overlay(&snap, &base, &tr).map(|s| s.reference),
            Some("popup_dialog.modal_dialog_popup")
        );
        snap.flow = PurchaseFlow::Done(PurchaseDialog::Success { title: "a".into() });
        assert!(
            overlay(&snap, &base, &tr).is_none(),
            "a success is a toast, not a modal"
        );
    }

    #[test]
    fn failures_show_the_vanilla_connection_text() {
        let mut snap = snapshot(StoreView::Home);
        snap.failure = Some(StoreError::Unavailable);
        let data = layout_data(&snap, &tr);
        assert_eq!(data.globals["#store_error_visible"], Scalar::Bool(true));
        assert_eq!(
            data.globals["#store_failure_text"],
            text("<store.connection.failed.body>")
        );
    }

    #[test]
    fn the_balance_wallet_text_is_available_for_tooltips() {
        assert_eq!(
            balance_text(5, &|_| "You Have %s Minecoins".to_owned()),
            "You Have 5 Minecoins"
        );
    }
}
