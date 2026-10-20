//! Narrow projection of Xbox activity observations into the existing friend-world cards.
//!
//! Listing neither establishes session membership nor authorizes a connection. Opaque
//! selected handles remain attached to their rows; malformed observations are skipped.

use bedrock_services::xbox::social::Activity;
use launcher::menu::view::{CatalogFriend, MenuFriendCard};
use serde_json::{Map, Value};
use std::{borrow::Cow, collections::HashSet};
use uuid::Uuid;

use super::world_metadata::{JSON_RPC_SIGNALING, NETHERNET_TRANSPORT, WEBSOCKET_SIGNALING, field};

/// One visible world and its optional complete, generation-bound native selection.
/// Display fields never supply transport authority.
pub struct NativeFriendWorld {
    /// Existing menu projection for this same row.
    card: MenuFriendCard,
    /// An opaque original service selection; unavailable observations cannot start a join.
    selection: Option<super::NativeFriendSelection>,
}

impl NativeFriendWorld {
    /// Binds a projection to the selection produced from that same service observation.
    pub(super) fn new(
        card: MenuFriendCard,
        selection: Option<super::NativeFriendSelection>,
    ) -> Self {
        Self { card, selection }
    }
    /// Borrows display-only fields without exposing or manufacturing session admission metadata.
    pub fn card(&self) -> &MenuFriendCard {
        &self.card
    }
    /// Copies the opaque selected owner for one caller-driven join.
    pub fn selection(&self) -> Option<super::NativeFriendSelection> {
        self.selection.clone()
    }
}

impl From<MenuFriendCard> for NativeFriendWorld {
    /// Display-only records cannot acquire a native service selection.
    fn from(card: MenuFriendCard) -> Self {
        Self {
            card,
            selection: None,
        }
    }
}

pub(super) struct ListedFriend {
    pub(super) card: MenuFriendCard,
    pub(super) selection: Option<super::FriendSelection>,
}

/// Preserves service order and the current menu's one-row-per-host/world de-duplication.
#[cfg(test)]
pub(super) fn cards(activities: Vec<Activity>, self_xuid: &str) -> Vec<MenuFriendCard> {
    listed(activities, self_xuid)
        .into_iter()
        .map(|world| world.card)
        .collect()
}

/// Keeps opaque selection metadata attached to its projected row through filtering and de-duplication.
pub(super) fn listed(activities: Vec<Activity>, self_xuid: &str) -> Vec<ListedFriend> {
    let mut seen = HashSet::new();
    let mut skipped = 0_usize;
    let values = activities
        .into_iter()
        .filter_map(|activity| {
            let selection = super::FriendSelection::activity(&activity).ok();
            let Some(card) = card(activity, self_xuid) else {
                skipped += 1;
                return None;
            };
            seen.insert((card.xuid.clone(), card.world_name.clone()))
                .then_some(ListedFriend { card, selection })
        })
        .collect();
    if skipped != 0 {
        tracing::debug!(
            skipped,
            "ignored unavailable friend-world activity observations"
        );
    }
    values
}

/// Applies the pinned catalog's listed-world and usable-signaling rules to fields this menu consumes.
fn card(activity: Activity, self_xuid: &str) -> Option<MenuFriendCard> {
    // The pinned activity owner defaults an absent/null closed flag to false.
    if activity.related_info?.closed.unwrap_or(false) {
        return None;
    }
    let custom = activity.custom_properties?;
    let world = custom.as_object()?;
    let host = text(world, "hostName")?;
    let owner = text(world, "ownerId")?;
    let name = text(world, "worldName")?;
    let members = integer(world, "MemberCount")?;
    let maximum = integer(world, "MaxMemberCount")?;
    if host.is_empty()
        || owner.is_empty()
        || owner == self_xuid
        || members <= 0
        || maximum < 0
        || integer(world, "RealmId")? != 0
        || !uuid(world, "ExperienceId")?.is_nil()
        || !uuid(world, "ExperienceWorldId")?.is_nil()
        || !text(world, "FriendId")?.is_empty()
        || !matches!(integer(world, "BroadcastSetting")?, 2..=4)
        || integer(world, "TransportLayer")? != NETHERNET_TRANSPORT
    {
        return None;
    }
    let Value::Array(connections) = field(world, "SupportedConnections")? else {
        return None;
    };
    let mut supported = false;
    for value in connections {
        supported |= usable_connection(value)?;
    }
    if !supported {
        return None;
    }
    Some(
        CatalogFriend {
            gamertag: host.into(),
            world_name: name.into(),
            xuid: owner.into(),
            members: i32::try_from(members).ok()?,
            max_members: i32::try_from(maximum).ok()?,
        }
        .into(),
    )
}

/// Accepts an advertised signaling method without interpreting it as an admitted native join.
/// Direct WebSocket uses RakNetGUID on the wire; JSON-RPC requires its own ID plus a nonnil PmsgId.
fn usable_connection(value: &Value) -> Option<bool> {
    if value.is_null() {
        return Some(false);
    }
    let connection = value.as_object()?;
    let kind = integer(connection, "ConnectionType")?;
    // Decode the recognized fields even on an auxiliary method, matching malformed-record rejection.
    let legacy = text(connection, "RakNetGUID")?;
    let messaging = uuid(connection, "PmsgId")?;
    let network = match field(connection, "NetherNetId") {
        None | Some(Value::Null) => Cow::Borrowed(""),
        Some(Value::String(value)) => Cow::Borrowed(value.as_str()),
        Some(value) => Cow::Owned(value.to_string()),
    };
    Some(match kind {
        WEBSOCKET_SIGNALING => usable_id(legacy),
        JSON_RPC_SIGNALING => usable_id(&network) && !messaging.is_nil(),
        _ => false,
    })
}

/// Accepts canonical nonzero decimal network IDs or UUID text, preserving the baseline's opaque-ID rule.
fn usable_id(value: &str) -> bool {
    value
        .parse::<u64>()
        .is_ok_and(|id| id != 0 && id.to_string() == value)
        || Uuid::parse_str(value).is_ok()
}

/// Missing/null title strings retain their zero value; a supplied wrong type rejects this observation.
fn text<'a>(object: &'a Map<String, Value>, name: &str) -> Option<&'a str> {
    match field(object, name) {
        None | Some(Value::Null) => Some(""),
        Some(value) => value.as_str(),
    }
}

/// Reads the signed integer metadata representation, without coercing strings or fractions.
fn integer(object: &Map<String, Value>, name: &str) -> Option<i64> {
    match field(object, name) {
        None | Some(Value::Null) => Some(0),
        Some(value) => value.as_i64(),
    }
}

/// Retains missing/null UUID zero values while rejecting malformed supplied UUIDs.
fn uuid(object: &Map<String, Value>, name: &str) -> Option<Uuid> {
    match field(object, name) {
        None | Some(Value::Null) => Some(Uuid::nil()),
        Some(Value::String(value)) => Uuid::parse_str(value).ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
