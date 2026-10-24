//! Account feed requests and replies shared by host workers and menu adapters.

use bridge::{Home, Message};

use launcher::menu::{
    auth::AuthState,
    view::{
        ButtonArt, InboxItem, LiveEventCard, MenuHome, MenuProfile, MenuRealmCard, MenuServerCard,
        PingInfo, ServerDetails,
    },
};

/// A request ticket, acceptance flag, and result from the Realm membership worker.
pub type RealmMembershipResponse = (u64, bool, Result<(String, MenuRealmCard), ()>);

/// A featured card and its original selection are delivered together.
/// Service display metadata alone does not authorize an allocated connection.
pub struct FeaturedRow {
    /// Existing menu card, including its presentation address.
    pub card: MenuServerCard,
    /// Existing information-panel data.
    pub details: ServerDetails,
    /// Original account-bound allocation selection, when this native catalog row needs one.
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    pub gathering: Option<crate::native_account::NativeGatheringSelection>,
}

impl From<(MenuServerCard, ServerDetails)> for FeaturedRow {
    /// Preserves ordinary presentation cards without manufacturing native service authority.
    fn from((card, details): (MenuServerCard, ServerDetails)) -> Self {
        Self {
            card,
            details,
            #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
            gathering: None,
        }
    }
}

/// Account presentation and explicit service requests needed by the menu.
pub trait AccountControl {
    /// The selected account's current sign-in presentation, when known.
    fn account_status(&mut self) -> Option<AuthState>;
    /// Changes whenever the account identity or sign-in state retires its data.
    fn account_generation(&mut self) -> Option<u64> {
        None
    }
    /// Queues sign-out retirement; true once the original owner accepted it.
    fn sign_out(&mut self) -> bool;
    /// Acquired featured cards and their original selection authority.
    fn featured(&mut self) -> Option<Vec<FeaturedRow>> {
        None
    }
    /// Requests live experience counts only while selected experience details are visible.
    fn set_player_counts_visible(&mut self, _visible: bool) {}
    /// The acquired signed-in profile.
    fn profile(&mut self) -> Option<MenuProfile> {
        None
    }
    /// Requests an immediate profile retry from the feed worker.
    fn refresh_profile(&mut self) {}
    /// The start screen's acquired service data.
    fn home(&mut self) -> Option<MenuHome> {
        None
    }
    /// Reports an inbox interaction through its original native service session.
    fn report_message(&mut self, _event: bridge::MessageEvent) {}
    /// The server rows kept fresh while the launcher shows them.
    fn set_ping_targets(&mut self, _targets: Vec<String>) {}
    /// Pongs from the latest ping round, keyed by address.
    fn pings(&mut self) -> Option<Vec<(String, PingInfo)>> {
        None
    }
    /// Whether the menu is connecting, which speeds up event polling.
    fn set_joining(&mut self, _joining: bool) {}
    /// Requests the selected account's Xbox friends off the frame.
    fn request_people(&mut self) {}
    /// The friends the last request listed, or `Err` when it failed.
    fn people(&mut self) -> Option<Result<Vec<launcher::menu::invite::Friend>, ()>> {
        None
    }
    /// Sends invites through the captured actual native hosting incarnation.
    fn send_invites(&mut self, _xuids: Vec<String>) {}
    /// Queues invitation preview or acceptance on the dedicated account worker.
    fn request_realm_membership(&mut self, _ticket: u64, _code: String, _accept: bool) -> bool {
        false
    }
    /// Retires a pending preview without retrying a possibly dispatched acceptance.
    fn cancel_realm_membership(&mut self) {}
    /// Delivers one request generation's result.
    fn realm_membership(&mut self) -> Option<RealmMembershipResponse> {
        None
    }
}

/// Projects acquired account Home data into the menu without service I/O.
pub fn menu_home(home: &Home, now_unix: i64) -> MenuHome {
    let art = |surface: &str| {
        home.messages
            .iter()
            .find(|message| message.surface == surface)
            .map(button_art)
    };
    let live_event = home
        .live_events
        .iter()
        .find(|event| event.end_unix == 0 || now_unix < event.end_unix)
        .map(|event| LiveEventCard {
            button_text: if event.button_text.is_empty() {
                "gathering.button.liveEventFallback".to_owned()
            } else {
                event.button_text.clone()
            },
            caption: event.caption_text.clone(),
            countdown: event.caption_countdown,
            start_unix: event.start_unix,
            badge_path: event.badge.path.clone(),
            address: event.address.clone(),
            route_to_servers: event.route_to_servers,
        });
    MenuHome {
        message_session: None,
        play_art: art(bridge::contract::MESSAGE_SURFACE_PLAY_BUTTON),
        inbox_unread: home
            .inbox
            .categories
            .iter()
            .filter(|category| launcher::menu::inbox::category_index(&category.kind).is_some())
            .fold(0_u32, |total, category| {
                total.saturating_add(category.unread)
            }),
        inbox_counts: home
            .inbox
            .categories
            .iter()
            .filter_map(|category| {
                Some((
                    launcher::menu::inbox::category_index(&category.kind)?,
                    category.unread,
                ))
            })
            .collect(),
        realm_invites: home.realm_invites,
        live_event,
        persona_head: home.persona_head.path.clone(),
        inbox: home
            .messages
            .iter()
            .filter(|message| {
                message.surface == bridge::contract::MESSAGE_SURFACE_INBOX_MESSAGE
                    && launcher::menu::inbox::category_index(&message.category).is_some()
            })
            .map(|message| InboxItem {
                instance_id: message.instance_id.clone(),
                report_id: message.report_id.clone(),
                received: message.received.clone(),
                source: message.sender.clone(),
                header: message.header.clone(),
                body: message.body.clone(),
                category: message.category.clone(),
                unread: !message
                    .status
                    .eq_ignore_ascii_case(bridge::contract::MESSAGE_STATUS_READ),
            })
            .collect(),
    }
}

/// Sorts a tile's images into the button's layers by their ids (hover, foreground).
pub(crate) fn button_art(message: &Message) -> ButtonArt {
    let mut art = ButtonArt {
        banner: message.banner.clone(),
        colors: message.colors.clone(),
        ..ButtonArt::default()
    };
    for image in message.images.iter().filter(|image| !image.path.is_empty()) {
        let id = image.id.to_ascii_lowercase();
        if id.contains(bridge::contract::MESSAGE_ART_BANNER) {
            art.banner_texture = image.path.clone();
            continue;
        }
        let hover = id.contains(bridge::contract::MESSAGE_ART_HOVER);
        let foreground = id.contains(bridge::contract::MESSAGE_ART_FORE)
            || id.contains(bridge::contract::MESSAGE_ART_FG);
        let slot = match (hover, foreground) {
            (true, true) => &mut art.hover_foreground,
            (true, false) => &mut art.hover_background,
            (false, true) => &mut art.default_foreground,
            (false, false) => &mut art.default_background,
        };
        if slot.is_empty() {
            *slot = image.path.clone();
        }
    }
    art
}
