//! Native empty-message illustrations in a bordered, category-specific card.

use super::super::super::menu_screens::Translate;
use super::super::paint::Bounds;
use super::super::theme::{BORDER, CAPTION, EDGE, NEUTRAL80, SECONDARY_BUTTON, TEXT, TEXT_DIMMEST};
use super::{Canvas, UiPresentationError};
use crate::ui_runtime::oreui_assets::INBOX_EMPTY_IMAGES;

const COPY: [(&str, &str, &str, &str); 5] = [
    (
        "emptyNewsTitle",
        "No new messages",
        "emptyNewsText",
        "This is where you’ll receive news and other updates about Minecraft. Check back soon for new messages.",
    ),
    (
        "emptyRealmSubTitle",
        "No new messages",
        "emptyRealmSubText",
        "This is where you'll receive messages about Realms, including feature updates and account information.",
    ),
    (
        "emptyInvitesTitle",
        "No new invites",
        "emptyInvitesText",
        "This is where your invites will show up when someone invites you to a Realm.",
    ),
    (
        "emptyMarketplacePassTitle",
        "Stay tuned for upcoming content!",
        "emptyMarketplacePassText",
        "This is where you'll receive messages about your Marketplace Pass subscription.",
    ),
    (
        "feedbackSubTitle",
        "Minecraft would like to hear from you!",
        "",
        "",
    ),
];

pub(super) fn draw(
    canvas: &mut Canvas<'_>,
    category: usize,
    bounds: Bounds,
    translate: Translate<'_>,
) -> Result<f32, UiPresentationError> {
    let index = category.min(COPY.len() - 1);
    let (title_key, title, caption_key, caption) = COPY[index];
    let title = translate(&format!("hbui.InboxRoute.{title_key}"))
        .map_or_else(|| title.to_owned(), |text| text.to_string());
    let caption = translate(&format!("hbui.InboxRoute.{caption_key}"))
        .map_or_else(|| caption.to_owned(), |text| text.to_string());
    let pad = canvas.r(1.6);
    let width = (bounds[2] - bounds[0] - pad * 2.0).max(1.0);
    let image_width = (256.0 * super::super::icons::native_scale(canvas)).min(width);
    let image_height = image_width * 96.0 / 256.0;
    let title_height = canvas.measure_height(&title, width, SECONDARY_BUTTON)?;
    let caption_height = if caption.is_empty() {
        0.0
    } else {
        canvas.measure_height(&caption, width, CAPTION)?
    };
    let bottom = bounds[1] + pad * 4.0 + title_height + image_height + caption_height;
    let panel = [bounds[0], bounds[1], bounds[2], bottom];
    canvas.fill(panel, NEUTRAL80.fill)?;
    canvas.frame(panel, EDGE, BORDER)?;
    let inner = [panel[0] + pad, panel[1] + pad, panel[2] - pad];
    canvas.centered_wrapped_text(&title, [inner[0], inner[1]], width, SECONDARY_BUTTON, TEXT)?;
    let top = inner[1] + title_height + pad;
    let left = (panel[0] + panel[2] - image_width) * 0.5;
    if !canvas.sprite(
        INBOX_EMPTY_IMAGES[index],
        [left, top, left + image_width, top + image_height],
        [255; 4],
    )? {
        let side = super::super::icons::native_side(canvas);
        super::icons::category_icon(
            canvas,
            index,
            [
                (panel[0] + panel[2] - side) * 0.5,
                top + (image_height - side) * 0.5,
            ],
        )?;
    }
    if !caption.is_empty() {
        let top = top + image_height + pad;
        canvas.centered_wrapped_text(&caption, [inner[0], top], width, CAPTION, TEXT_DIMMEST)?;
    }
    Ok(bottom)
}
