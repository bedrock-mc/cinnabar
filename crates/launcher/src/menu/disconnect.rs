//! What the disconnect screen says about how a session ended: vanilla's
//! `disconnectionScreen.*`/`disconnect.*` wording for each failure kind, or the
//! server's own kick message. The raw error chain only reaches the log.

/// The screen's title and body.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisconnectText {
    /// A lang key and vanilla's English text for it.
    pub title: (&'static str, &'static str),
    pub body: DisconnectBody,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DisconnectBody {
    /// A lang key.
    Key(&'static str),
    /// A server-sent kick message, shown as sent (with § codes).
    Server(String),
}

const DISCONNECTED: (&str, &str) = (
    "disconnectionScreen.disconnected",
    "Disconnected from Server",
);
const CONNECT_FAILED: (&str, &str) = ("connect.failed", "Failed to connect to the server");
/// The prefix the session puts before a server's disconnect reason.
const SERVER_PREFIX: &str = "server disconnected: ";

/// The error chain for a disconnect the launcher core reported with the
/// server's `reason` (empty when the server sent none).
pub fn from_server(reason: &str) -> String {
    format!("{SERVER_PREFIX}{reason} (launcher core)")
}

/// Vanilla's wording for a session that ended with `error`.
pub fn describe(error: &str) -> DisconnectText {
    let text = |title, body| DisconnectText { title, body };
    if let Some(rest) = error.trim().strip_prefix(SERVER_PREFIX) {
        // "<reason> (<transport>)": the reason is everything before the last " (".
        let reason = rest
            .rsplit_once(" (")
            .map_or(rest, |(reason, _)| reason)
            .trim();
        return text(DISCONNECTED, server_reason(reason));
    }
    let lower = error.to_ascii_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|needle| lower.contains(needle));
    let body = if has(&["timed out", "timeout"]) {
        "disconnect.timeout"
    } else if has(&["resource pack", "pack download", "downloadpack"]) {
        "disconnectionScreen.resourcePack"
    } else if has(&["outdated client", "client outdated", "client is outdated"]) {
        "disconnectionScreen.outdatedClient"
    } else if has(&["outdated server", "server outdated", "server is outdated"]) {
        "disconnectionScreen.outdatedServer"
    } else if has(&[
        "not authenticated",
        "authentication",
        "login failed",
        "xsts",
        "xbox live",
    ]) {
        "disconnectionScreen.notAuthenticated"
    } else if has(&[
        "connection refused",
        "unreachable",
        "no route",
        "dial",
        "lookup",
        "resolve",
        "bridge connection failed",
    ]) {
        return text(
            CONNECT_FAILED,
            DisconnectBody::Key("disconnectionScreen.cantConnect"),
        );
    } else if has(&["closed", "end of stream"]) {
        "disconnect.closed"
    } else {
        "disconnectionScreen.noReason"
    };
    text(DISCONNECTED, DisconnectBody::Key(body))
}

/// Lang keys a server (the core, on a failed join) may send as its whole message.
pub const SERVER_SENT_KEYS: &[&str] = &bridge::contract::CORE_DISCONNECT_KEYS;

/// English for a body key when the loaded packs lack it: Cinnabar's own keys carry their text,
/// and a vanilla key reads as itself, as before.
pub fn body_fallback(key: &'static str) -> &'static str {
    match key {
        bridge::contract::DISCONNECT_KEY_DEVICE_CLOCK => DEVICE_CLOCK_TEXT,
        _ => key,
    }
}

const DEVICE_CLOCK_TEXT: &str = concat!(
    "Your device's clock is behind, so sign-in was rejected. ",
    "Turn on automatic date and time, then try again."
);

/// A server's disconnect text: a known `DisconnectFailReason` name reads as
/// vanilla's line for it, a known lang key is localized as vanilla does, and
/// any other text is the server's own message.
fn server_reason(reason: &str) -> DisconnectBody {
    if let Some(key) = SERVER_SENT_KEYS.iter().find(|key| **key == reason) {
        return DisconnectBody::Key(key);
    }
    let key = match reason {
        "" | "Unknown" | "NoReason" => "disconnectionScreen.noReason",
        "TimedOut" | "Timeout" => "disconnect.timeout",
        "ServerFull" => "disconnectionScreen.serverFull",
        "OutdatedClient" => "disconnectionScreen.outdatedClient",
        "OutdatedServer" => "disconnectionScreen.outdatedServer",
        "NotAuthenticated" => "disconnectionScreen.notAuthenticated",
        "NotAllowed" => "disconnectionScreen.notAllowed",
        "LoggedInOtherLocation" => "disconnectionScreen.loggedinOtherLocation",
        "Kicked" => "disconnect.kicked",
        "ResourcePackProblem" => "disconnectionScreen.resourcePack",
        "InvalidSkin" => "disconnectionScreen.invalidSkin",
        "Closed" | "Shutdown" => "disconnect.closed",
        _ => return DisconnectBody::Server(reason.to_owned()),
    };
    DisconnectBody::Key(key)
}

/// Recognizes the existing outdated-client reason without treating arbitrary server text as consent.
pub fn requires_client_update(error: &str) -> bool {
    const KEY: &str = "disconnectionScreen.outdatedClient";
    match describe(error).body {
        DisconnectBody::Key(key) => key == KEY,
        DisconnectBody::Server(text) => text == KEY,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_outdated_client_reason_requires_a_client_update() {
        for error in [
            "server rejected the handshake: outdated client".to_owned(),
            from_server("OutdatedClient"),
            from_server("disconnectionScreen.outdatedClient"),
        ] {
            assert!(requires_client_update(&error));
        }
        for error in [
            "server rejected the handshake: outdated server".to_owned(),
            from_server("Please update your resource packs"),
            "read timed out".to_owned(),
        ] {
            assert!(!requires_client_update(&error));
        }
    }

    // Every failure kind reads as vanilla words it; a kick keeps the server's text.
    #[test]
    fn failure_kinds_map_to_vanilla_text() {
        let body = |error: &str| describe(error).body;
        let key = DisconnectBody::Key;
        assert_eq!(
            body("network session failed: Bedrock session failed: Connection closed"),
            key("disconnect.closed")
        );
        assert_eq!(
            body("network session failed: read timed out"),
            key("disconnect.timeout")
        );
        assert_eq!(
            body("server disconnected: §cYou were banned (network read failed: closed)"),
            DisconnectBody::Server("§cYou were banned".to_owned())
        );
        assert_eq!(
            body("server disconnected: ServerFull (connection closed)"),
            key("disconnectionScreen.serverFull")
        );
        assert_eq!(
            body("resource pack download failed: stalled"),
            key("disconnectionScreen.resourcePack")
        );
        assert_eq!(
            body("login failed: XSTS token rejected"),
            key("disconnectionScreen.notAuthenticated")
        );
        assert_eq!(
            body("server rejected the handshake: outdated client"),
            key("disconnectionScreen.outdatedClient")
        );
        assert_eq!(
            body("server rejected the handshake: outdated server"),
            key("disconnectionScreen.outdatedServer")
        );
        let unreachable = describe("bridge connection failed: Connection refused (os error 61)");
        assert_eq!(unreachable.title, CONNECT_FAILED);
        assert_eq!(unreachable.body, key("disconnectionScreen.cantConnect"));
        assert_eq!(body("something odd"), key("disconnectionScreen.noReason"));
        assert_eq!(describe("closed").title, DISCONNECTED);
    }

    // Every key the core sends reads as that key, and Cinnabar's own keys have English text.
    #[test]
    fn core_sent_keys_read_as_their_text() {
        for &sent in SERVER_SENT_KEYS {
            assert_eq!(describe(&from_server(sent)).body, DisconnectBody::Key(sent));
            if sent.starts_with("cinnabar.") {
                assert_ne!(body_fallback(sent), sent, "{sent} has no English text");
            } else {
                assert_eq!(body_fallback(sent), sent);
            }
        }
    }
}
