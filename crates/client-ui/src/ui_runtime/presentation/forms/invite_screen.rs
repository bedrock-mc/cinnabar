//! Vanilla's invite screen over the hosted world: the account's Xbox friends in its online and
//! offline lists with gamertag, gamerpic and checkbox, and the send button's pick count. The
//! platform, linked-account and party lists stay hidden, as vanilla hides them when absent.

use json_ui::{CollectionItem, Context, DataSource, HitKind, HitRegion, Scalar};
use launcher::menu::invite::{Action, Friend, InviteState, Section};
use serde_json::Value;

use super::menu_screens::{Translate, flags, text, translated};
use crate::menu::{MenuAction, MenuView};

/// One of vanilla's Xbox Live friend lists and the globals its category binds.
struct List {
    section: Section,
    collection: &'static str,
    visible: &'static str,
    dimension: &'static str,
}

const LISTS: [List; 2] = [
    List {
        section: Section::Online,
        collection: "online_xbox_live_friends",
        visible: "#online_xbox_live_friends_visible",
        dimension: "#online_xbox_live_friend_grid_dimension",
    },
    List {
        section: Section::Offline,
        collection: "offline_xbox_live_friends",
        visible: "#offline_xbox_live_friends_visible",
        dimension: "#offline_xbox_live_friend_grid_dimension",
    },
];

const NO_FRIENDS: &str = "invite.noFriends";
const LOAD_ERROR: &str = "invite.error1";
const SEND: &str = "invite.sendUnnumbered";
const SEND_ONE: &str = "invite.sendOne";
const SEND_COUNT: &str = "invite.send";

/// The screen-creation vars vanilla's invite controller sets for a game invite on desktop: no
/// Realm, member management or pagination, and the load error only when the list failed.
pub(super) fn context(view: &MenuView, context: Context) -> Context {
    let failed = view.invite.as_deref().is_some_and(InviteState::failed);
    [
        ("is_inviting_to_realm", false),
        ("can_manage_members", false),
        ("use_pagination", false),
        ("use_vertical_button_stack_panel", false),
        ("invite_platform_icons_visible", false),
        ("hide_err_message", !failed),
    ]
    .into_iter()
    .fold(context, |context, (name, value)| {
        context.with_flag(name, value)
    })
    .with_var("err_message_text", Value::String(LOAD_ERROR.to_owned()))
}

pub(super) fn bind(view: &MenuView, data: &mut DataSource, translate: Translate<'_>) {
    let default = InviteState::default();
    let state = view.invite.as_deref().unwrap_or(&default);
    data.set_global("#is_loading", Scalar::Bool(state.loading()));
    flags(data, &["#cross_platform_enabled"]);
    let mut listed = 0;
    for list in &LISTS {
        let rows: Vec<CollectionItem> = state
            .section(list.section)
            .map(|friend| row(state, friend))
            .collect();
        listed += rows.len();
        data.set_global(list.visible, Scalar::Bool(!rows.is_empty()));
        data.set_grid_dimensions(list.dimension, [1, rows.len() as u32]);
        data.set_collection(list.collection, rows);
    }
    let empty = !state.loading() && !state.failed() && listed == 0;
    data.set_global("#no_xbox_live_friends_visible", Scalar::Bool(empty));
    data.set_global(
        "#no_xbox_live_friends_text",
        text(translated(translate, NO_FRIENDS, NO_FRIENDS)),
    );
    data.set_global(
        "#send_button",
        text(send_label(state.selected_count(), translate)),
    );
}

/// A friend row; `#profile_image_options` names the gamerpic the row's renderer draws.
fn row(state: &InviteState, friend: &Friend) -> CollectionItem {
    CollectionItem::default()
        .with("#xbl_gamertag", text(friend.gamertag.clone()))
        .with("#current_game_label", text(""))
        .with("#online_visible", Scalar::Bool(friend.online))
        .with("#offline_visible", Scalar::Bool(!friend.online))
        .with("#profile_image_options", text(friend.picture_path.clone()))
        .with(
            "#toggle_invite_state",
            Scalar::Bool(state.is_selected(friend)),
        )
}

/// "Send Invites" until a friend is picked, then the count.
fn send_label(picked: usize, translate: Translate<'_>) -> String {
    match picked {
        0 => translated(translate, SEND, SEND),
        1 => translated(translate, SEND_ONE, SEND_ONE),
        count => {
            translated(translate, SEND_COUNT, SEND_COUNT).replacen("%d", &count.to_string(), 1)
        }
    }
}

/// A friend row's checkbox or the send button; closing falls through to the shared exit.
pub(super) fn action(region: &HitRegion) -> Option<MenuAction> {
    let action = if region.kind == HitKind::Toggle {
        let list = LISTS
            .iter()
            .find(|list| region.collection.as_deref() == Some(list.collection))?;
        Action::Toggle(list.section, region.collection_index?)
    } else if region.pressed.as_deref() == Some("button.send_invites") {
        Action::Send
    } else {
        return None;
    };
    Some(MenuAction::Invite(action))
}

#[cfg(test)]
mod tests;
