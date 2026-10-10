//! The play route's Realms tab: the side menu in four of twelve columns (Add
//! or join with pending invites, then your and joined Realms) and the selected
//! Realm's details in eight (10:3 image, name and tags, the hero Play button).

use super::super::super::UiPresentationError;
use super::grid::{Grid, space};
use super::paint::{Bounds, Canvas};
use super::theme::{BODY, NEUTRAL80, NEUTRAL100, SECONDARY_BUTTON, TEXT, TEXT_DARK, TEXT_DIMMER};
use super::widgets::{Variant, button, row, row_text, section_label, side_menu, tag};
use launcher::menu::{MenuAction, MenuRealmCard, MenuView};

/// Owner and invited tag fill.
const PRIMARY_TINT: [u8; 4] = [0x6c, 0xc3, 0x49, 255];
/// Closed and expired tag fill (needs the OreUI reference).
const WARNING_TINT: [u8; 4] = [0xd0, 0x3c, 0x3c, 255];

pub(super) fn draw(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    grid: &Grid,
    body: Bounds,
) -> Result<(), UiPresentationError> {
    let (menu_span, details_span) = if grid.narrow {
        ((0, 3), (3, 5))
    } else {
        ((0, 4), (4, 8))
    };
    let [menu_left, menu_right] = grid.span(menu_span.0, menu_span.1);
    side_menu(canvas, [menu_left, body[1], menu_right, body[3]])?;
    let pad = canvas.r(1.6);
    let scroll = canvas.begin_scroll("realms_list", [menu_left, body[1], menu_right, body[3]])?;
    let mut y = body[1] + space(canvas, 2) - scroll.offset;
    let add_height = canvas.r(4.4);
    button(
        canvas,
        view,
        [menu_left + pad, y, menu_right - pad, y + add_height],
        Variant::Secondary,
        "Add/join Realm",
        (view.auth_state == launcher::menu::auth::AuthState::Authenticated).then_some(
            MenuAction::RealmMembership(launcher::menu::realm_membership::Action::Open),
        ),
    )?;
    y += add_height;
    let invites = view.feeds.home.realm_invites;
    if invites > 0 {
        y = section_label(
            canvas,
            &format!("Realm invites ({invites})"),
            [menu_left, menu_right],
            y,
        )?;
    }
    let item_height = canvas.r(4.8);
    let selected = view
        .feeds
        .selected_realm
        .filter(|index| *index < view.realms.len());
    let mut first = None;
    for (label, member) in [("Your Realms", false), ("Joined Realms", true)] {
        let realms: Vec<(usize, &MenuRealmCard)> = view
            .realms
            .iter()
            .enumerate()
            .filter(|(_, realm)| realm.member == member)
            .collect();
        y = section_label(
            canvas,
            &format!("{label} ({})", realms.len()),
            [menu_left, menu_right],
            y,
        )?;
        for (index, realm) in realms {
            first.get_or_insert(index);
            if y + item_height < body[1] || y > body[3] {
                y += item_height;
                continue;
            }
            let bounds = [
                menu_left + canvas.r(0.2),
                y,
                menu_right - canvas.r(0.2),
                y + item_height,
            ];
            row(
                canvas,
                view,
                bounds,
                selected.or(first) == Some(index),
                Some(MenuAction::SelectRealm(index)),
            )?;
            let text_width = bounds[2] - bounds[0] - pad * 2.0;
            let detail = if member {
                realm.owner.as_str()
            } else {
                realm.state.as_str()
            };
            row_text(
                canvas,
                [bounds[0] + pad, y],
                text_width,
                &realm.name,
                detail,
            )?;
            y += item_height;
        }
    }
    let content = y + scroll.offset - body[1];
    canvas.end_scroll(scroll, content)?;
    let [left, right] = grid.span(details_span.0, details_span.1);
    let panel = [left, body[1], right, body[3]];
    canvas.fill(panel, NEUTRAL80.fill)?;
    canvas.frame(panel, 0.2, [0x1e, 0x1e, 0x1f, 255])?;
    let Some(index) = selected.or(first) else {
        canvas.text_centred(
            "No Realms yet",
            [left, body[1], right, body[1] + canvas.r(10.0)],
            SECONDARY_BUTTON,
            TEXT,
            false,
        )?;
        return Ok(());
    };
    let realm = &view.realms[index];
    let edge = canvas.r(0.2);
    let image = [
        left + edge,
        body[1] + edge,
        right - edge,
        body[1] + edge + (right - left) * 0.3,
    ];
    canvas.fill(image, NEUTRAL100)?;
    let players = format!("{}/{}", realm.online_players, realm.max_players);
    let overlay = [image[0], image[3] - canvas.r(4.0), image[2], image[3]];
    canvas.fill(overlay, [0, 0, 0, 179])?;
    canvas.text(
        &players,
        [overlay[0] + canvas.r(1.6), overlay[1] + canvas.r(1.0)],
        overlay[2] - overlay[0],
        BODY,
        TEXT_DIMMER,
        false,
    )?;
    let pad = canvas.r(2.4);
    let mut y = image[3] + space(canvas, 3);
    y += canvas.text(
        &realm.name,
        [left + pad, y],
        right - left - pad * 2.0,
        BODY,
        TEXT,
        false,
    )? + space(canvas, 1);
    let owner_tag = if realm.member { "Invited" } else { "Owner" };
    let mut x = tag(canvas, owner_tag, [left + pad, y], PRIMARY_TINT, TEXT_DARK)?;
    let open = realm.can_play();
    if !open {
        let state = if realm.expired { "Expired" } else { "Closed" };
        x = tag(canvas, state, [x + canvas.r(0.8), y], WARNING_TINT, TEXT)?;
    }
    let detail = realm_detail(realm);
    if !detail.is_empty() {
        canvas.text(
            &detail,
            [x + canvas.r(0.8), y],
            right - pad - x,
            BODY,
            TEXT_DIMMER,
            false,
        )?;
    }
    y += canvas.r(2.0) + space(canvas, 3);
    let play_width = canvas.r(32.0).min((right - left) * 0.5);
    button(
        canvas,
        view,
        [left + pad, y, left + pad + play_width, y + canvas.r(4.4)],
        Variant::Hero,
        "Play",
        open.then_some(MenuAction::PlayRealm(index)),
    )
}

/// The owner of a joined Realm, or the time left on an owned one.
fn realm_detail(realm: &MenuRealmCard) -> String {
    if realm.member {
        return if realm.owner.is_empty() {
            String::new()
        } else {
            format!("Owner: {}", realm.owner)
        };
    }
    match realm.days_left {
        _ if realm.expired => String::new(),
        1 => "1 day left".to_owned(),
        days if days > 1 => format!("{days} days left"),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use {super::*, launcher::menu::MenuRealmCard};

    fn realm(member: bool, days_left: i32, expired: bool) -> MenuRealmCard {
        MenuRealmCard {
            name: "Realm".to_owned(),
            state: "open".to_owned(),
            target: String::new(),
            address: String::new(),
            owner: "Alex".to_owned(),
            online_players: 0,
            max_players: 10,
            days_left,
            expired,
            member,
        }
    }

    #[test]
    fn details_name_the_owner_or_the_time_left() {
        assert_eq!(realm_detail(&realm(true, 0, false)), "Owner: Alex");
        assert_eq!(realm_detail(&realm(false, 21, false)), "21 days left");
        assert_eq!(realm_detail(&realm(false, 1, false)), "1 day left");
        assert_eq!(realm_detail(&realm(false, 5, true)), "");
    }
}
