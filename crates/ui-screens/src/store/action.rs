//! What a press on a store screen means. Buttons are the vanilla pressed ids; a card's row and
//! position come from its region, since factory rows and grids nest their own collections.

use json_ui::HitRegion;

use launcher::store::snapshot::StoreView;

use launcher::store::StoreAction;

/// Factory rows lead with the header bar item before the displayed rows.
const LEADING_STATIC_ITEMS: usize = 1;

/// The bracketed collection indices in a region key, outermost first.
pub fn key_indices(key: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let mut rest = key;
    while let Some(start) = rest.find('[') {
        let Some(end) = rest[start..].find(']') else {
            break;
        };
        if let Ok(index) = rest[start + 1..start + end].parse() {
            out.push(index);
        }
        rest = &rest[start + end + 1..];
    }
    out
}

/// The action for a pressed `region` while `view` shows and `modal` is up.
pub fn action_for(view: StoreView, modal: bool, region: &HitRegion) -> Option<StoreAction> {
    let pressed = region.pressed.as_deref()?;
    if modal {
        return match pressed {
            "popup_dialog.left_button" | "popup_dialog.middle_button" => {
                Some(StoreAction::ModalPrimary)
            }
            "popup_dialog.rightcancel_button" => Some(StoreAction::ModalSecondary),
            "button.menu_exit" | "popup_dialog.escape" => Some(StoreAction::ModalDismiss),
            _ => None,
        };
    }
    Some(match pressed {
        "button.menu_exit" => StoreAction::Back,
        "button.homeButton" => StoreAction::Home,
        "button.search" => StoreAction::OpenSearch,
        "button.coin_wallet" | "button.purchase_coins" => StoreAction::CoinWallet,
        "button.my_account" => StoreAction::Inventory,
        "button.purchase_with_coins" | "button.interact_button" if view == StoreView::Detail => {
            StoreAction::Buy
        }
        "button.select_offer" => {
            let index = region.collection_index?;
            let path = key_indices(&region.key);
            // Rows nest their offers; a single-list view has no enclosing row index.
            let row = if matches!(view, StoreView::Home) {
                path.first()?.checked_sub(LEADING_STATIC_ITEMS)?
            } else {
                0
            };
            StoreAction::OpenOffer {
                row: u16::try_from(row).ok()?,
                index: u16::try_from(index).ok()?,
            }
        }
        "button.navigate_next_page" if view == StoreView::Search => {
            StoreAction::ShowMore { row: 0 }
        }
        "button.show_more_offers" => {
            let row = match view {
                StoreView::Home => key_indices(&region.key)
                    .first()?
                    .checked_sub(LEADING_STATIC_ITEMS)?,
                _ => 0,
            };
            StoreAction::ShowMore {
                row: u16::try_from(row).ok()?,
            }
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use json_ui::{HitKind, RectOut};

    use {super::*, launcher::store::StoreAction, launcher::store::snapshot::StoreView};

    fn region(pressed: &str, key: &str, index: Option<usize>) -> HitRegion {
        let rect = RectOut {
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
        };
        HitRegion {
            key: key.to_owned(),
            name: "b".to_owned(),
            kind: HitKind::Button,
            rect,
            clip: rect,
            layer: 0,
            order: 0,
            pressed: Some(pressed.to_owned()),
            control_name: None,
            collection_index: index,
            collection: None,
            enabled: true,
            checked: None,
            max_length: None,
            group_index: None,
            renderer: None,
            drag_axes: [false; 2],
            sound: None,
            input: Default::default(),
            focus: None,
            widget: Default::default(),
            collections: Vec::new(),
            modal_root: None,
        }
    }

    #[test]
    fn key_indices_read_every_bracket_in_order() {
        assert_eq!(key_indices("/a[3]/b/c[12]/d[x]/e[0]"), [3, 12, 0]);
        assert!(key_indices("/plain/path").is_empty());
    }

    #[test]
    fn an_offer_card_resolves_its_row_past_the_header_item() {
        let card = region(
            "button.select_offer",
            "/s/rows[3]/row/offers[2]/card",
            Some(2),
        );
        assert_eq!(
            action_for(StoreView::Home, false, &card),
            Some(StoreAction::OpenOffer { row: 2, index: 2 })
        );
        let header = region("button.select_offer", "/s/rows[0]/x", Some(0));
        assert_eq!(action_for(StoreView::Home, false, &header), None);
        let grid = region("button.select_offer", "/s/grid/card[5]", Some(5));
        assert_eq!(
            action_for(StoreView::Search, false, &grid),
            Some(StoreAction::OpenOffer { row: 0, index: 5 })
        );
    }

    #[test]
    fn modal_presses_map_only_to_modal_actions() {
        let left = region("popup_dialog.left_button", "/m/l", None);
        let back = region("button.menu_exit", "/m/x", None);
        assert_eq!(
            action_for(StoreView::Detail, true, &left),
            Some(StoreAction::ModalPrimary)
        );
        assert_eq!(
            action_for(StoreView::Detail, true, &back),
            Some(StoreAction::ModalDismiss)
        );
        assert_eq!(action_for(StoreView::Detail, false, &left), None);
        let buy = region("button.purchase_with_coins", "/b", None);
        assert_eq!(action_for(StoreView::Detail, true, &buy), None);
        assert_eq!(
            action_for(StoreView::Detail, false, &buy),
            Some(StoreAction::Buy)
        );
        assert_eq!(action_for(StoreView::Home, false, &buy), None);
    }
}
