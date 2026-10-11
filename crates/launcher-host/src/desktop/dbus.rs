//! Linux desktop calls over the session bus: the xdg-desktop-portal and the notification service.

use std::{
    collections::HashMap,
    path::PathBuf,
    pin::pin,
    sync::atomic::{AtomicU32, Ordering},
    time::Duration,
};

use futures_lite::{Stream, StreamExt, future};
use zbus::{
    Connection, MatchRule, MessageStream, Proxy,
    message::Type,
    zvariant::{OwnedObjectPath, OwnedValue, Value},
};

const PORTAL: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const REQUEST: &str = "org.freedesktop.portal.Request";
/// Whole-request deadlines, covering connect, the call and the Response; the chooser allows for
/// a user still browsing. Past them the tool fallbacks take over.
const CHOOSER_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const OPEN_URI_TIMEOUT: Duration = Duration::from_secs(30);
const NOTIFY_TIMEOUT: Duration = Duration::from_secs(5);
/// Responses buffered while the call is in flight; other apps' requests can share the bus.
const RESPONSE_QUEUE: usize = 64;

type Results = HashMap<String, OwnedValue>;

/// Opens `uri` in the desktop's default handler; `false` means the chooser was dismissed.
pub fn open_uri(uri: &str) -> zbus::Result<bool> {
    let (code, _) = request(
        "org.freedesktop.portal.OpenURI",
        "OpenURI",
        uri,
        HashMap::new(),
        OPEN_URI_TIMEOUT,
    )?;
    opened(code)
}

/// Preserves cancellation so each caller can report it without retrying another handler.
fn opened(code: u32) -> zbus::Result<bool> {
    response("OpenURI", code)
}

/// Returns the chosen local file; `None` when the user dismissed the chooser.
pub fn pick_file(
    title: &str,
    filter_name: &str,
    patterns: &[String],
) -> zbus::Result<Option<PathBuf>> {
    let globs = patterns
        .iter()
        .map(|pattern| (0u32, pattern.as_str()))
        .collect::<Vec<_>>();
    let mut options = HashMap::new();
    options.insert("filters", Value::from(vec![(filter_name, globs)]));
    let (code, results) = request(
        "org.freedesktop.portal.FileChooser",
        "OpenFile",
        title,
        options,
        CHOOSER_TIMEOUT,
    )?;
    chosen_file(code, results)
}

fn chosen_file(code: u32, mut results: Results) -> zbus::Result<Option<PathBuf>> {
    if !response("OpenFile", code)? {
        return Ok(None);
    }
    let uris = results
        .remove("uris")
        .map(Vec::<String>::try_from)
        .transpose()?
        .unwrap_or_default();
    Ok(uris.first().and_then(|uri| file_path(uri)))
}

/// Shows a desktop notification, as `notify-send` would.
pub fn notify(summary: &str, body: &str) -> zbus::Result<()> {
    async_io::block_on(with_deadline("Notify", NOTIFY_TIMEOUT, async {
        let connection = Connection::session().await?;
        let proxy = Proxy::new(
            &connection,
            "org.freedesktop.Notifications",
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
        )
        .await?;
        let hints: HashMap<&str, Value> = HashMap::new();
        let _: u32 = proxy
            .call(
                "Notify",
                &(
                    launcher::PRODUCT_NAME,
                    0u32,
                    "",
                    summary,
                    body,
                    Vec::<&str>::new(),
                    hints,
                    -1i32,
                ),
            )
            .await?;
        Ok(())
    }))
}

/// Calls a portal method shaped `(parent_window, argument, options)`; returns its response code
/// (0 success, 1 cancelled, 2 other) and results, or an error once `timeout` passes.
fn request(
    interface: &str,
    method: &str,
    argument: &str,
    mut options: HashMap<&str, Value<'_>>,
    timeout: Duration,
) -> zbus::Result<(u32, Results)> {
    static NEXT_TOKEN: AtomicU32 = AtomicU32::new(0);
    let token = format!(
        "cinnabar_{}_{}",
        std::process::id(),
        NEXT_TOKEN.fetch_add(1, Ordering::Relaxed)
    );
    options.insert("handle_token", Value::from(token));
    async_io::block_on(with_deadline(method, timeout, async {
        let connection = Connection::session().await?;
        // Every Response is buffered from before the call, so one that beats the returned
        // handle (older portals ignore the token and choose their own path) is still matched.
        let rule = MatchRule::builder()
            .msg_type(Type::Signal)
            .interface(REQUEST)?
            .member("Response")?
            .path_namespace(format!("{PORTAL_PATH}/request"))?
            .build();
        let responses = MessageStream::for_match_rule(rule, &connection, Some(RESPONSE_QUEUE))
            .await?
            .map(|message| {
                let message = message?;
                let path = message.header().path().map(ToString::to_string);
                Ok((path, message))
            });
        let portal = Proxy::new(&connection, PORTAL, PORTAL_PATH, interface).await?;
        let handle: OwnedObjectPath = portal.call(method, &("", argument, options)).await?;
        response_for(responses, handle.as_str())
            .await?
            .body()
            .deserialize()
    }))
}

/// Runs `work`, failing once `timeout` passes so a silent bus peer cannot block the caller.
async fn with_deadline<T>(
    what: &str,
    timeout: Duration,
    work: impl Future<Output = zbus::Result<T>>,
) -> zbus::Result<T> {
    future::or(work, async {
        async_io::Timer::after(timeout).await;
        Err(zbus::Error::Failure(format!(
            "{what} got no answer within {timeout:?}"
        )))
    })
    .await
}

/// The first event addressed to `handle`, skipping other requests' responses.
async fn response_for<T>(
    events: impl Stream<Item = zbus::Result<(Option<String>, T)>>,
    handle: &str,
) -> zbus::Result<T> {
    let mut events = pin!(events);
    while let Some(event) = events.next().await {
        let (path, value) = event?;
        if path.as_deref() == Some(handle) {
            return Ok(value);
        }
    }
    Err(zbus::Error::Failure(
        "portal request closed without a response".into(),
    ))
}

/// Whether the user completed the request; `false` when they dismissed it, `Err` when the
/// portal failed, so callers can fall back to another route.
fn response(method: &str, code: u32) -> zbus::Result<bool> {
    match code {
        0 => Ok(true),
        1 => Ok(false),
        code => Err(zbus::Error::Failure(format!(
            "{method} ended with response {code}"
        ))),
    }
}

fn file_path(uri: &str) -> Option<PathBuf> {
    url::Url::parse(uri).ok()?.to_file_path().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_deadline_fails_a_request_that_never_answers() {
        let silent = with_deadline(
            "OpenFile",
            Duration::from_millis(20),
            future::pending::<zbus::Result<()>>(),
        );
        let error = async_io::block_on(silent).unwrap_err().to_string();
        assert!(error.contains("OpenFile got no answer"), "{error}");
        let prompt = with_deadline("OpenFile", Duration::from_secs(60), async { Ok(7) });
        assert_eq!(async_io::block_on(prompt).unwrap(), 7);
    }

    #[test]
    fn a_response_that_arrives_before_the_handle_is_known_still_matches() {
        let handle = "/org/freedesktop/portal/desktop/request/1_9/legacy";
        let other = "/org/freedesktop/portal/desktop/request/1_3/other";
        let events = futures_lite::stream::iter([
            Ok((Some(other.to_owned()), "other app")),
            Ok((None, "no path")),
            Ok((Some(handle.to_owned()), "ours")),
            Ok((Some(handle.to_owned()), "late duplicate")),
        ]);
        assert_eq!(
            async_io::block_on(response_for(events, handle)).unwrap(),
            "ours"
        );
        let unrelated = futures_lite::stream::iter([Ok((Some(other.to_owned()), "other app"))]);
        assert!(async_io::block_on(response_for(unrelated, handle)).is_err());
    }

    #[test]
    fn open_uri_preserves_cancellation_and_failures() {
        assert!(opened(0).unwrap());
        assert!(!opened(1).unwrap());
        assert!(opened(2).is_err());
    }

    #[test]
    fn file_chooser_separates_cancel_from_failure() {
        let uris = |uris: &[&str]| {
            let value = Value::from(uris.iter().map(|uri| uri.to_string()).collect::<Vec<_>>());
            Results::from([("uris".to_owned(), OwnedValue::try_from(value).unwrap())])
        };
        assert_eq!(
            chosen_file(0, uris(&["file:///packs/a.mcpack"])).unwrap(),
            Some(PathBuf::from("/packs/a.mcpack"))
        );
        assert_eq!(
            chosen_file(1, uris(&["file:///packs/a.mcpack"])).unwrap(),
            None
        );
        assert!(chosen_file(2, Results::new()).is_err());
    }

    #[test]
    fn only_local_file_uris_become_paths() {
        assert_eq!(
            file_path("file:///home/dev/My%20Pack.mcpack"),
            Some(PathBuf::from("/home/dev/My Pack.mcpack"))
        );
        assert_eq!(file_path("https://example.test/pack.mcpack"), None);
        assert_eq!(file_path("not a uri"), None);
    }
}
