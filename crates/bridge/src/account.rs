use std::path::Path;

use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::endpoint::EndpointKind;
use crate::status::{CONTROL_MAX_FRAME_LEN, RpcError, TransferPending, invalid};
use crate::{BridgeError, FramedStream};

const REQUEST_ID: u64 = 1;
const SCHEMA_VERSION: u32 = 1;

/// One Realm the account can join; `target` is its stable join id.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct Realm {
    pub name: String,
    pub state: String,
    pub target: String,
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub owner: String,
    #[serde(default)]
    pub motd: String,
    #[serde(default)]
    pub world_type: String,
    #[serde(default)]
    pub online_players: u32,
    #[serde(default)]
    pub max_players: u32,
    #[serde(default)]
    pub days_left: i32,
    #[serde(default)]
    pub expired: bool,
    /// Joined as a member rather than owned.
    #[serde(default)]
    pub member: bool,
}

/// Remote HTTPS artwork and the core's cached copy of it, when it has one.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct Artwork {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub path: String,
}

/// One activity a featured server or gathering advertises.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct FeaturedGame {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub subtitle: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub image: Artwork,
}

/// A featured server with the details the play screen's info panel shows.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct FeaturedServer {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub caption: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub news_title: String,
    #[serde(default)]
    pub news: String,
    #[serde(default)]
    pub logo: Artwork,
    #[serde(default)]
    pub screenshots: Vec<Artwork>,
    #[serde(default)]
    pub games: Vec<FeaturedGame>,
}

/// A community gathering; the core joins it by `id` only when the player connects.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct Gathering {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub caption: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub creator: String,
    #[serde(default)]
    pub image: Artwork,
    #[serde(default)]
    pub start_unix: i64,
    #[serde(default)]
    pub end_unix: i64,
}

/// The signed-in account as the start and profile screens show it.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct Profile {
    #[serde(default)]
    pub gamertag: String,
    #[serde(default)]
    pub xuid: String,
    #[serde(default)]
    pub gamerpic: Artwork,
    #[serde(default)]
    pub avatar: Artwork,
    #[serde(default)]
    pub avatar_error: bool,
    #[serde(default)]
    pub featured_screenshot: Artwork,
    #[serde(default)]
    pub featured_screenshot_error: bool,
    #[serde(default)]
    pub real_name: String,
    #[serde(default)]
    pub presence_text: String,
    /// Absent when the lookup failed, so the UI never shows a fabricated zero.
    #[serde(default)]
    pub gamerscore: Option<i64>,
    #[serde(default)]
    pub friends: Option<u32>,
    #[serde(default)]
    pub followers: Option<u32>,
    #[serde(default)]
    pub statistics: Option<ProfileStatistics>,
    #[serde(default)]
    pub achievements: Option<ProfileAchievements>,
}

/// The four Xbox title statistics requested by vanilla's PlayerStatisticsFacet.
/// Numeric strings preserve the service's precision; absent values are unavailable.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct ProfileStatistics {
    pub minutes_played: Option<String>,
    pub blocks_broken: Option<String>,
    pub mobs_defeated: Option<String>,
    pub distance_travelled: Option<String>,
}

/// The account's achievement summary for the authenticated Minecraft title.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct ProfileAchievements {
    pub unlocked: u32,
    pub total: u32,
    pub current_gamerscore: Option<i64>,
    pub max_gamerscore: Option<i64>,
    pub entries: Vec<ProfileAchievement>,
}

/// Xbox achievement data; game-specific suggested order remains optional.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct ProfileAchievement {
    pub id: String,
    pub name: String,
    pub description: String,
    pub image: Artwork,
    pub gamerscore: Option<i64>,
    pub locked: bool,
    #[serde(default)]
    pub date_unlocked: String,
    pub suggested_order: Option<u32>,
}

/// The start screen's service data: messaging surfaces, inbox counts,
/// treatments, the pending Realms invite count, live events and the persona head.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Home {
    #[serde(default)]
    pub messages: Vec<Message>,
    #[serde(default)]
    pub inbox: Inbox,
    #[serde(default)]
    pub treatments: Vec<String>,
    #[serde(default)]
    pub realm_invites: u32,
    #[serde(default)]
    pub live_events: Vec<LiveEvent>,
    #[serde(default)]
    pub persona_head: Artwork,
}

/// One player-messaging message; `surface` places it (`PlayButton`, `InboxMessage`, ...).
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct Message {
    #[serde(default)]
    pub colors: std::collections::BTreeMap<String, [u8; 3]>,
    #[serde(default)]
    pub received: String,
    #[serde(default)]
    pub sender: String,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub instance_id: String,
    #[serde(default)]
    pub report_id: String,
    #[serde(default)]
    pub surface: String,
    #[serde(default)]
    pub template: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub header: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub sub_title: String,
    #[serde(default)]
    pub banner: String,
    #[serde(default)]
    pub images: Vec<MessageImage>,
    #[serde(default)]
    pub buttons: Vec<MessageButton>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct MessageImage {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub path: String,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct MessageButton {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub link: String,
    #[serde(default)]
    pub action: String,
}

/// Per-category service totals can include messages outside the loaded page.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct InboxCategory {
    #[serde(default, rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub unread: u32,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct Inbox {
    #[serde(default)]
    pub categories: Vec<InboxCategory>,
    #[serde(default)]
    pub total: u32,
    #[serde(default)]
    pub unread: u32,
}

/// A live gathering with its active segment's start-screen UI.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct LiveEvent {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub start_unix: i64,
    #[serde(default)]
    pub end_unix: i64,
    #[serde(default)]
    pub route_to_servers: bool,
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub button_text: String,
    #[serde(default)]
    pub caption_text: String,
    #[serde(default)]
    pub caption_countdown: bool,
    #[serde(default)]
    pub badge: Artwork,
    #[serde(default)]
    pub event_image: Artwork,
}

/// A messaging report: `event_type` is Click, Dismiss, Delete, Impression, ControlImpression or ReadAll.
#[derive(Clone, Debug, Default, Serialize, Eq, PartialEq)]
pub struct MessageEvent {
    pub event_type: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub instance_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub report_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub button_id: String,
}

#[derive(Deserialize)]
struct HomeBody {
    #[serde(default)]
    home: Home,
}

/// One server's RakNet pong; `online` is false when it did not answer.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct ServerPing {
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub online: bool,
    #[serde(default)]
    pub players: u32,
    #[serde(default)]
    pub max_players: u32,
    #[serde(default)]
    pub ping_ms: u32,
    #[serde(default)]
    pub motd: String,
}

#[derive(Serialize)]
struct PingParams<'a> {
    addresses: &'a [String],
}

#[derive(Deserialize)]
struct PingBody {
    #[serde(default)]
    servers: Vec<ServerPing>,
}

/// One friend's joinable world; `xuid` identifies it for [`ConnectTarget::Friend`].
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct Friend {
    pub gamertag: String,
    pub xuid: String,
    pub world_name: String,
    pub members: u32,
    pub max_members: u32,
    #[serde(default)]
    pub handle_id: Option<String>,
    #[serde(default)]
    pub address: Option<String>,
}

/// Where the next client connection goes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConnectTarget {
    /// A `host:port` server.
    RakNet(String),
    /// A Realm id from [`Realm::target`] (`realm_id/<id>` → `<id>`).
    Realm(String),
    /// A friend's XUID from [`Friend::xuid`].
    Friend(String),
    /// A gathering's experience ID from [`Gathering::id`].
    Gathering(String),
}

impl ConnectTarget {
    fn params(&self) -> ConnectParams<'_> {
        let (kind, value) = match self {
            Self::RakNet(value) => ("raknet", value),
            Self::Realm(value) => ("realm", value),
            Self::Friend(value) => ("friend", value),
            Self::Gathering(value) => ("gathering", value),
        };
        ConnectParams { kind, value }
    }
}

#[derive(Serialize)]
struct ConnectParams<'a> {
    kind: &'static str,
    value: &'a str,
}

/// Sign-in state of the core.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AuthState {
    /// The core runs without a Microsoft account.
    Offline,
    SignedOut,
    /// Show `verification_uri` and `user_code` to the player.
    AwaitingCode,
    SignedIn,
    Failed,
}

/// Sign-in state plus the fields that belong to it; `reason` never carries secrets or paths.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct Account {
    pub state: AuthState,
    #[serde(default)]
    pub verification_uri: Option<String>,
    #[serde(default)]
    pub user_code: Option<String>,
    #[serde(default)]
    pub gamertag: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

/// The server's own reason for ending a session; `sequence` grows with every disconnect.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct ServerDisconnect {
    pub reason: i32,
    pub message: String,
    pub sequence: u64,
}

/// Pollable core events: auth state plus the newest disconnect and pending transfer.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct Events {
    pub auth: Account,
    #[serde(default)]
    pub disconnect: Option<ServerDisconnect>,
    #[serde(default)]
    pub transfer: Option<TransferPending>,
    /// Live while the core prepares a join; gone once it hands the session to the client.
    #[serde(default)]
    pub connect: Option<ConnectProgress>,
}

/// The core's stage of preparing a join, and its pack download counts.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
pub struct ConnectProgress {
    pub stage: ConnectStage,
    /// Packs being downloaded (cache hits excluded) and those finished.
    #[serde(default)]
    pub packs_done: u32,
    #[serde(default)]
    pub packs_total: u32,
    /// Across all packs; the total grows as each pack's download begins.
    #[serde(default)]
    pub received_bytes: u64,
    #[serde(default)]
    pub total_bytes: u64,
}

/// Vanilla's join progress handlers the core's stages stand for.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ConnectStage {
    /// The Realm lookup.
    Realm,
    /// Resource pack acquisition.
    Packs,
    /// Transport connect and login; also any stage this client does not know.
    #[serde(other)]
    Connecting,
}

#[derive(Serialize)]
struct Request<'a, P: Serialize> {
    jsonrpc: &'static str,
    id: u64,
    method: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<P>,
}

#[derive(Deserialize)]
struct Envelope<R> {
    jsonrpc: String,
    id: u64,
    #[serde(default = "none")]
    result: Option<Versioned<R>>,
    #[serde(default)]
    error: Option<RpcError>,
}

fn none<T>() -> Option<T> {
    None
}

#[derive(Deserialize)]
struct Versioned<R> {
    schema_version: u32,
    #[serde(flatten)]
    body: R,
}

#[derive(Deserialize)]
struct Empty {}

#[derive(Deserialize)]
struct RealmsBody {
    realms: Vec<Realm>,
}

#[derive(Deserialize)]
struct FriendsBody {
    friends: Vec<Friend>,
}

#[derive(Deserialize)]
struct AccountBody {
    account: Account,
}

#[derive(Deserialize)]
struct FeaturedBody {
    #[serde(default)]
    servers: Vec<FeaturedServer>,
}

#[derive(Deserialize)]
struct GatheringsBody {
    #[serde(default)]
    gatherings: Vec<Gathering>,
}

#[derive(Deserialize)]
struct ProfileBody {
    #[serde(default)]
    profile: Profile,
}

pub(crate) async fn call<R: DeserializeOwned, P: Serialize>(
    socket_dir: &Path,
    method: &str,
    params: Option<P>,
) -> Result<R, BridgeError> {
    let stream = crate::endpoint::connect(socket_dir, EndpointKind::Control).await?;
    let mut framed = FramedStream::with_max(stream, CONTROL_MAX_FRAME_LEN);
    let request = serde_json::to_vec(&Request {
        jsonrpc: "2.0",
        id: REQUEST_ID,
        method,
        params,
    })?;
    framed.send(Bytes::from(request)).await?;
    let response = framed.next().await.ok_or(BridgeError::ControlClosed)??;
    parse_response(&response)
}

pub(crate) fn parse_response<R: DeserializeOwned>(payload: &[u8]) -> Result<R, BridgeError> {
    let response: Envelope<R> = serde_json::from_slice(payload)?;
    if response.jsonrpc != "2.0" {
        return invalid("jsonrpc must be exactly 2.0");
    }
    if response.id != REQUEST_ID {
        return invalid("response id does not match the request");
    }
    match (response.result, response.error) {
        (Some(result), None) if result.schema_version == SCHEMA_VERSION => Ok(result.body),
        (Some(_), None) => invalid("unsupported launcher schema version"),
        (None, Some(error)) => Err(BridgeError::ControlRpc {
            code: error.code,
            message: error.message,
        }),
        (Some(_), Some(_)) => invalid("response contains both result and error"),
        (None, None) => invalid("response contains neither result nor error"),
    }
}

/// Lists the account's Realms.
pub async fn list_realms(socket_dir: &Path) -> Result<Vec<Realm>, BridgeError> {
    let body: RealmsBody = call::<_, ()>(socket_dir, "realms_list.v1", None).await?;
    Ok(body.realms)
}

/// Lists friends' joinable worlds.
pub async fn list_friends(socket_dir: &Path) -> Result<Vec<Friend>, BridgeError> {
    let body: FriendsBody = call::<_, ()>(socket_dir, "friends_list.v1", None).await?;
    Ok(body.friends)
}

/// Selects the upstream for the next game-socket connection.
pub async fn connect_target(socket_dir: &Path, target: &ConnectTarget) -> Result<(), BridgeError> {
    call::<Empty, _>(socket_dir, "connect.v1", Some(target.params())).await?;
    Ok(())
}

/// Warms only the selected server's transport; `None` cancels it without selecting a join.
pub async fn prepare_connect_target(
    socket_dir: &Path,
    target: Option<&ConnectTarget>,
) -> Result<(), BridgeError> {
    let params = target.map_or(
        ConnectParams {
            kind: "",
            value: "",
        },
        ConnectTarget::params,
    );
    call::<Empty, _>(socket_dir, "prepare_connect.v1", Some(params)).await?;
    Ok(())
}

/// Reads the sign-in state.
pub async fn account_status(socket_dir: &Path) -> Result<Account, BridgeError> {
    let body: AccountBody = call::<_, ()>(socket_dir, "account_status.v1", None).await?;
    Ok(body.account)
}

/// Deletes the cached Microsoft tokens and returns the resulting state.
pub async fn sign_out(socket_dir: &Path) -> Result<Account, BridgeError> {
    let body: AccountBody = call::<_, ()>(socket_dir, "sign_out.v1", None).await?;
    Ok(body.account)
}

/// Reads the auth state and the newest disconnect and transfer.
pub async fn poll_events(socket_dir: &Path) -> Result<Events, BridgeError> {
    call::<Events, ()>(socket_dir, "events.v1", None).await
}

/// Lists the featured servers.
pub async fn list_featured_servers(socket_dir: &Path) -> Result<Vec<FeaturedServer>, BridgeError> {
    let body: FeaturedBody = call::<_, ()>(socket_dir, "featured_servers.v1", None).await?;
    Ok(body.servers)
}

/// Lists the community gatherings.
pub async fn list_gatherings(socket_dir: &Path) -> Result<Vec<Gathering>, BridgeError> {
    let body: GatheringsBody = call::<_, ()>(socket_dir, "gatherings.v1", None).await?;
    Ok(body.gatherings)
}

/// Addresses one `ping.v1` request may carry (the core's `catalog.MaxPingTargets`);
/// a longer list was refused whole, so no row ever left "Loading ping".
const MAX_PING_TARGETS: usize = 64;

/// Pings servers for their player counts and round trip, in batches the core accepts.
pub async fn ping_servers(
    socket_dir: &Path,
    addresses: &[String],
) -> Result<Vec<ServerPing>, BridgeError> {
    let mut servers = Vec::with_capacity(addresses.len());
    for batch in addresses.chunks(MAX_PING_TARGETS) {
        let params = PingParams { addresses: batch };
        let body: PingBody = call(socket_dir, "ping.v1", Some(params)).await?;
        servers.extend(body.servers);
    }
    Ok(servers)
}

/// Reads the start screen's service data.
pub async fn home(socket_dir: &Path) -> Result<Home, BridgeError> {
    let body: HomeBody = call::<_, ()>(socket_dir, "home.v1", None).await?;
    Ok(body.home)
}

/// Reports one messaging event (impression, click, dismiss, ...).
pub async fn report_message_event(
    socket_dir: &Path,
    event: &MessageEvent,
) -> Result<(), BridgeError> {
    call::<Empty, _>(socket_dir, "message_event.v1", Some(event)).await?;
    Ok(())
}

/// Reads the signed-in profile.
pub async fn profile(socket_dir: &Path) -> Result<Profile, BridgeError> {
    let body: ProfileBody = call::<_, ()>(socket_dir, "profile.v1", None).await?;
    Ok(body.profile)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connect_params_match_the_wire_contract() {
        let encoded = serde_json::to_value(&Request {
            jsonrpc: "2.0",
            id: 1,
            method: "connect.v1",
            params: Some(ConnectTarget::Realm("42".into()).params()),
        })
        .expect("encode");
        assert_eq!(
            encoded,
            serde_json::json!({"jsonrpc":"2.0","id":1,"method":"connect.v1","params":{"kind":"realm","value":"42"}})
        );
        let raknet = serde_json::to_value(ConnectTarget::RakNet("a:1".into()).params());
        assert_eq!(
            raknet.expect("encode"),
            serde_json::json!({"kind":"raknet","value":"a:1"})
        );
        let friend = serde_json::to_value(ConnectTarget::Friend("9".into()).params());
        assert_eq!(
            friend.expect("encode"),
            serde_json::json!({"kind":"friend","value":"9"})
        );
        let gathering = serde_json::to_value(ConnectTarget::Gathering("e".into()).params());
        assert_eq!(
            gathering.expect("encode"),
            serde_json::json!({"kind":"gathering","value":"e"})
        );
    }

    #[test]
    fn parses_realm_and_friend_lists() {
        let realms = br#"{"jsonrpc":"2.0","id":1,"result":{"schema_version":1,"realms":[
            {"name":"R","state":"OPEN","target":"realm_id/7","address":"1.2.3.4:19132"}]}}"#;
        let body: RealmsBody = parse_response(realms).expect("realms");
        assert_eq!(body.realms[0].target, "realm_id/7");
        let friends = br#"{"jsonrpc":"2.0","id":1,"result":{"schema_version":1,"friends":[
            {"gamertag":"F","xuid":"123","world_name":"W","members":1,"max_members":8}]}}"#;
        let body: FriendsBody = parse_response(friends).expect("friends");
        assert_eq!(body.friends[0].xuid, "123");
        assert_eq!(body.friends[0].handle_id, None);
        let empty = br#"{"jsonrpc":"2.0","id":1,"result":{"schema_version":1,"realms":[]}}"#;
        assert!(
            parse_response::<RealmsBody>(empty)
                .expect("empty")
                .realms
                .is_empty()
        );
    }

    #[test]
    fn parses_account_and_events() {
        let account = br#"{"jsonrpc":"2.0","id":1,"result":{"schema_version":1,
            "account":{"state":"awaiting_code","verification_uri":"https://x.test/l","user_code":"AB12"}}}"#;
        let body: AccountBody = parse_response(account).expect("account");
        assert_eq!(body.account.state, AuthState::AwaitingCode);
        assert_eq!(body.account.user_code.as_deref(), Some("AB12"));
        let events = br#"{"jsonrpc":"2.0","id":1,"result":{"schema_version":1,
            "auth":{"state":"signed_in","gamertag":"Steve"},
            "disconnect":{"reason":7,"message":"banned","sequence":2},
            "transfer":{"host":"n.example","port":19133,"sequence":1}}}"#;
        let events: Events = parse_response(events).expect("events");
        assert_eq!(events.auth.gamertag.as_deref(), Some("Steve"));
        assert_eq!(events.disconnect.expect("disconnect").sequence, 2);
        assert_eq!(events.transfer.expect("transfer").port, 19133);
        let quiet =
            br#"{"jsonrpc":"2.0","id":1,"result":{"schema_version":1,"auth":{"state":"offline"}}}"#;
        let quiet: Events = parse_response(quiet).expect("quiet");
        assert_eq!(quiet.auth.state, AuthState::Offline);
        assert!(quiet.disconnect.is_none() && quiet.transfer.is_none());
        assert!(quiet.connect.is_none());
    }

    // Omitted counts read as zero and an unknown stage reads as connecting.
    #[test]
    fn parses_connect_progress() {
        let events = |connect: &str| {
            let reply = format!(
                r#"{{"jsonrpc":"2.0","id":1,"result":{{"schema_version":1,
                "auth":{{"state":"signed_in"}},"connect":{connect}}}}}"#
            );
            parse_response::<Events>(reply.as_bytes())
                .expect("events")
                .connect
                .expect("connect")
        };
        let stage = |connect: &str| events(connect).stage;
        assert_eq!(stage(r#"{"stage":"realm"}"#), ConnectStage::Realm);
        assert_eq!(stage(r#"{"stage":"connecting"}"#), ConnectStage::Connecting);
        assert_eq!(
            stage(r#"{"stage":"handshaking"}"#),
            ConnectStage::Connecting
        );
        assert_eq!(
            events(r#"{"stage":"packs"}"#),
            ConnectProgress {
                stage: ConnectStage::Packs,
                packs_done: 0,
                packs_total: 0,
                received_bytes: 0,
                total_bytes: 0,
            }
        );
        let downloading = events(
            r#"{"stage":"packs","packs_done":1,"packs_total":3,
            "received_bytes":5242880,"total_bytes":20971520}"#,
        );
        assert_eq!(downloading.packs_done, 1);
        assert_eq!(downloading.packs_total, 3);
        assert_eq!(downloading.received_bytes, 5_242_880);
        assert_eq!(downloading.total_bytes, 20_971_520);
    }

    #[test]
    fn screen_feeds_parse_leniently() {
        let featured = br#"{"jsonrpc":"2.0","id":1,"result":{"schema_version":1,"servers":[
            {"name":"S","address":"a.test:19132","logo":{"url":"https://a.test/l.png"},
             "games":[{"title":"Skywars"}],"future":true},{}]}}"#;
        let body: FeaturedBody = parse_response(featured).expect("featured");
        assert_eq!(body.servers.len(), 2);
        assert_eq!(body.servers[0].logo.url, "https://a.test/l.png");
        assert_eq!(body.servers[0].games[0].title, "Skywars");
        let gatherings = br#"{"jsonrpc":"2.0","id":1,"result":{"schema_version":1}}"#;
        assert!(
            parse_response::<GatheringsBody>(gatherings)
                .expect("gatherings")
                .gatherings
                .is_empty()
        );
        let profile = br#"{"jsonrpc":"2.0","id":1,"result":{"schema_version":1,
            "profile":{"gamertag":"Steve","xuid":"1","gamerpic":{"path":"/art/p.img"}}}}"#;
        let body: ProfileBody = parse_response(profile).expect("profile");
        assert_eq!(body.profile.gamerpic.path, "/art/p.img");
        assert!(body.profile.statistics.is_none());
        let profile = br#"{"jsonrpc":"2.0","id":1,"result":{"schema_version":1,
            "profile":{"statistics":{"minutes_played":"120.5","blocks_broken":"0"}}}}"#;
        let body: ProfileBody = parse_response(profile).expect("profile statistics");
        let statistics = body.profile.statistics.expect("loaded statistics");
        assert_eq!(statistics.minutes_played.as_deref(), Some("120.5"));
        assert_eq!(statistics.blocks_broken.as_deref(), Some("0"));
        assert!(statistics.mobs_defeated.is_none());
        assert!(statistics.distance_travelled.is_none());
        let realm = br#"{"jsonrpc":"2.0","id":1,"result":{"schema_version":1,"realms":[
            {"name":"R","state":"OPEN","target":"realm_id/7","online_players":2,"max_players":10}]}}"#;
        let body: RealmsBody = parse_response(realm).expect("realm");
        assert_eq!(body.realms[0].online_players, 2);
    }

    #[test]
    fn ping_params_and_results_match_the_wire_contract() {
        let addresses = vec!["a.test:19132".to_owned()];
        let encoded = serde_json::to_value(&PingParams {
            addresses: &addresses,
        })
        .expect("encode");
        assert_eq!(encoded, serde_json::json!({"addresses":["a.test:19132"]}));
        let reply = br#"{"jsonrpc":"2.0","id":1,"result":{"schema_version":1,"servers":[
            {"address":"a.test:19132","online":true,"players":3,"max_players":20,"ping_ms":41}]}}"#;
        let body: PingBody = parse_response(reply).expect("ping");
        assert_eq!(body.servers[0].ping_ms, 41);
    }

    #[test]
    fn home_parses_leniently_and_events_skip_empty_fields() {
        let reply = br#"{"jsonrpc":"2.0","id":1,"result":{"schema_version":1,"home":{
            "messages":[{"id":"m","surface":"PlayButton","template":"t",
              "images":[{"id":"tile","url":"https://a.test/t.png","path":"/art/t.img"}]}],
            "inbox":{"total":3,"unread":2,"categories":[]},"realm_invites":1,
            "live_events":[{"id":"g","caption_countdown":true}],"extra":1}}}"#;
        let body: HomeBody = parse_response(reply).expect("home");
        assert_eq!(body.home.messages[0].images[0].path, "/art/t.img");
        assert_eq!(body.home.inbox.unread, 2);
        assert!(body.home.live_events[0].caption_countdown);
        let event = MessageEvent {
            event_type: "Impression".into(),
            instance_id: "i".into(),
            ..MessageEvent::default()
        };
        assert_eq!(
            serde_json::to_value(&event).expect("encode"),
            serde_json::json!({"event_type":"Impression","instance_id":"i"})
        );
    }

    #[test]
    fn surfaces_rpc_errors_and_rejects_bad_envelopes() {
        let error =
            br#"{"jsonrpc":"2.0","id":1,"error":{"code":-32020,"message":"Not signed in"}}"#;
        assert!(matches!(
            parse_response::<Empty>(error),
            Err(BridgeError::ControlRpc { code: -32020, .. })
        ));
        let wrong_schema = br#"{"jsonrpc":"2.0","id":1,"result":{"schema_version":2}}"#;
        assert!(parse_response::<Empty>(wrong_schema).is_err());
        let wrong_id = br#"{"jsonrpc":"2.0","id":9,"result":{"schema_version":1}}"#;
        assert!(parse_response::<Empty>(wrong_id).is_err());
        let neither = br#"{"jsonrpc":"2.0","id":1}"#;
        assert!(parse_response::<Empty>(neither).is_err());
    }
}

#[cfg(all(test, unix))]
mod prepare_connect_tests;
