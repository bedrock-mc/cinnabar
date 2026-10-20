//! Actual framed Core replies must retain the selected account source and presentation generation.

use super::*;
use crate::launcher_core::TitlePresenceOwner;
use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::{net::UnixListener, task::JoinHandle};

enum AtReply {
    FinishRound(TitlePresenceLifetime),
    ReplaceAccount,
}

/// Native signout retires its actual account owner without asking a foreign Core to mutate storage.
#[tokio::test]
async fn native_signout_retires_the_original_worker_without_a_core_signout_request() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let directory = tempfile::tempdir().unwrap();
        let listener = UnixListener::bind(bridge::control_endpoint_path(directory.path())).unwrap();
        let shared = Mutex::new(Snapshot::native(
            None,
            bridge::contract::DEFAULT_SERVICE_LANGUAGE,
        ));
        let lifetime = TitlePresenceLifetime::new();
        let (requests, receive) = bounded(1);
        requests.send(()).unwrap();
        let mut owner = Box::pin(async {
            let guard = lifetime.worker_guard();
            run(Some(directory.path()), &shared, &receive, &lifetime).await;
            drop(guard);
        });
        let foreign_request = async {
            let (stream, _) = listener.accept().await.unwrap();
            let mut framed = tokio_util::codec::Framed::new(
                stream,
                bridge::BridgeCodec::with_max(bridge::MAX_FRAME_LEN),
            );
            let request = framed.next().await.unwrap().unwrap();
            let request: serde_json::Value = serde_json::from_slice(&request).unwrap();
            panic!(
                "native signout dispatched a foreign Core mutation: {}",
                request["method"]
            );
        };
        tokio::select! { biased;
            _ = foreign_request => {},
            _ = owner.as_mut() => {},
        }
        assert_eq!(
            lifetime.retirement(),
            Some(TitlePresenceRetirement::Complete)
        );
    })
    .await
    .expect("native signout owner did not retire");
}

/// Presence policy cannot replace the actual Core account's signout request and response.
#[tokio::test]
async fn legacy_signout_preserves_core_auth_independently_of_presence_policy() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let directory = tempfile::tempdir().unwrap();
        let listener = UnixListener::bind(bridge::control_endpoint_path(directory.path())).unwrap();
        let shared = Mutex::new(Snapshot {
            title_presence_owner: TitlePresenceOwner::Native,
            ..Default::default()
        });
        let lifetime = TitlePresenceLifetime::new();
        let (requests, receive) = bounded(1);
        requests.send(()).unwrap();
        let server = async {
            let (stream, _) = listener.accept().await.unwrap();
            let mut framed = tokio_util::codec::Framed::new(
                stream,
                bridge::BridgeCodec::with_max(bridge::MAX_FRAME_LEN),
            );
            let request = framed.next().await.unwrap().unwrap();
            let request: serde_json::Value = serde_json::from_slice(&request).unwrap();
            assert_eq!(request["method"], bridge::contract::METHOD_SIGN_OUT);
            let reply = serde_json::json!({
                "jsonrpc":bridge::contract::JSON_RPC_VERSION,
                "id":request["id"],
                "result":{
                    "schema_version":bridge::contract::CONTROL_SCHEMA_VERSION,
                    "account":{"state":"signed_out"},
                },
            });
            framed
                .send(Bytes::from(serde_json::to_vec(&reply).unwrap()))
                .await
                .unwrap();
            lifetime.retire();
        };
        tokio::join!(
            server,
            control(Some(directory.path()), &shared, &receive, &lifetime),
            native_presence::run(&shared, &lifetime),
        );
        assert_eq!(
            lifetime.retirement(),
            Some(TitlePresenceRetirement::Complete)
        );
    })
    .await
    .expect("legacy signout control did not retire");
}

/// Native presence leaves the original Core account reply and legacy route observations intact.
#[tokio::test]
async fn core_account_auth_survives_native_presence_selection() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let directory = tempfile::tempdir().unwrap();
        let listener = UnixListener::bind(bridge::control_endpoint_path(directory.path())).unwrap();
        let shared = Arc::new(Mutex::new(Snapshot {
            title_presence_owner: TitlePresenceOwner::Native,
            last_disconnect: Some(1),
            ..Default::default()
        }));
        let mut peer = core_peer(
            listener,
            Arc::clone(&shared),
            AtReply::FinishRound(TitlePresenceLifetime::new()),
        );
        poll_core_events(directory.path(), &shared, true, &mut None).await;
        assert_eq!(peer.finish().await, [bridge::contract::METHOD_EVENTS]);
        let snapshot = shared.lock().unwrap();
        assert_eq!(
            snapshot.account.as_ref().unwrap().gamertag.as_deref(),
            Some("ForeignCore")
        );
        assert!(snapshot.connect.is_some() && snapshot.server_trust.is_some());
        assert!(matches!(
            snapshot.events.as_slice(),
            [AccountEvent::Disconnected { .. }]
        ));
    })
    .await
    .expect("original Core authentication reply did not complete");
}

/// One finite real control peer; cancellation keeps its original task abortable.
struct CorePeer(Option<JoinHandle<Vec<String>>>);

impl CorePeer {
    /// Returns the original task's actual methods only after its retained handler finishes.
    async fn finish(&mut self) -> Vec<String> {
        let methods = self.0.as_mut().unwrap().await.unwrap();
        self.0 = None;
        methods
    }
}

impl Drop for CorePeer {
    fn drop(&mut self) {
        if let Some(task) = &self.0 {
            task.abort();
        }
    }
}

/// Replies through the same framing and method envelope as a Core event/control endpoint.
fn core_peer(listener: UnixListener, shared: Arc<Mutex<Snapshot>>, at_reply: AtReply) -> CorePeer {
    CorePeer(Some(tokio::spawn(async move {
        let mut methods = Vec::new();
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let mut framed = tokio_util::codec::Framed::new(
                stream,
                bridge::BridgeCodec::with_max(bridge::MAX_FRAME_LEN),
            );
            let request = framed.next().await.unwrap().unwrap();
            let request: serde_json::Value = serde_json::from_slice(&request).unwrap();
            let method = request["method"].as_str().unwrap();
            methods.push(method.to_owned());
            let events = method == bridge::contract::METHOD_EVENTS;
            let result = if events {
                match &at_reply {
                    AtReply::FinishRound(lifetime) => lifetime.retire(),
                    AtReply::ReplaceAccount => {
                        let mut snapshot = shared.lock().unwrap();
                        snapshot.auth_generation += 1;
                        snapshot.account_source = AccountSource::Native;
                        snapshot.title_presence_owner = TitlePresenceOwner::Native;
                    }
                }
                serde_json::json!({
                    "schema_version":bridge::contract::CONTROL_SCHEMA_VERSION,
                    "auth":{"state":"signed_in","gamertag":"ForeignCore"},
                    "disconnect":{"reason":0,"message":"Legacy route ended","sequence":4},
                    "connect":{"stage":"connecting"},
                    "server_trust":{"id":42,"url":"https://synthetic.invalid"},
                })
            } else {
                assert_eq!(method, bridge::contract::METHOD_CINNABAR_API);
                serde_json::json!({"schema_version":bridge::contract::CONTROL_SCHEMA_VERSION})
            };
            let reply = serde_json::json!({"jsonrpc":bridge::contract::JSON_RPC_VERSION,
                "id":request["id"],"result":result});
            framed
                .send(Bytes::from(serde_json::to_vec(&reply).unwrap()))
                .await
                .unwrap();
            if events {
                return methods;
            }
        }
    })))
}

/// A completed legacy-route event still cannot configure or authenticate a missing native account.
#[tokio::test]
async fn native_account_auth_and_product_do_not_fall_back_to_core() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let directory = tempfile::tempdir().unwrap();
        let listener = UnixListener::bind(bridge::control_endpoint_path(directory.path())).unwrap();
        let shared = Arc::new(Mutex::new(Snapshot {
            native_product_api: None,
            cinnabar_api: Some(bridge::CinnabarApiConfig {
                client_version: env!("CARGO_PKG_VERSION").into(),
                client_build: 1,
                channel: "beta".into(),
                visible: true,
            }),
            last_disconnect: Some(1),
            ..Snapshot::native(None, bridge::contract::DEFAULT_SERVICE_LANGUAGE)
        }));
        let lifetime = TitlePresenceLifetime::new();
        let mut peer = core_peer(
            listener,
            Arc::clone(&shared),
            AtReply::FinishRound(lifetime.clone()),
        );
        let (_requests, receive) = bounded(1);
        tokio::join!(
            control(Some(directory.path()), &shared, &receive, &lifetime),
            native_presence::run(&shared, &lifetime),
        );
        let methods = peer.finish().await;
        let snapshot = shared.lock().unwrap();
        assert_eq!(
            lifetime.retirement(),
            Some(TitlePresenceRetirement::Complete)
        );
        assert!(snapshot.connect.is_some() && snapshot.server_trust.is_some());
        assert!(matches!(
            snapshot.events.as_slice(),
            [AccountEvent::Disconnected { .. }]
        ));
        assert!(
            snapshot.account.is_none(),
            "foreign Core authentication replaced the selected native account source"
        );
        assert_eq!(
            methods,
            [bridge::contract::METHOD_EVENTS],
            "a missing native Product owner configured the foreign Core account"
        );
    })
    .await
    .expect("finite native account/legacy route control fixture stalled");
}

/// A real in-flight Core response cannot publish route events into a replacement account.
#[tokio::test]
async fn replaced_account_discards_the_entire_original_core_event_response() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let directory = tempfile::tempdir().unwrap();
        let listener = UnixListener::bind(bridge::control_endpoint_path(directory.path())).unwrap();
        let shared = Arc::new(Mutex::new(Snapshot {
            auth_generation: 7,
            last_disconnect: Some(1),
            ..Default::default()
        }));
        let mut peer = core_peer(listener, Arc::clone(&shared), AtReply::ReplaceAccount);
        poll_core_events(directory.path(), &shared, true, &mut None).await;
        let methods = peer.finish().await;
        let snapshot = shared.lock().unwrap();
        assert_eq!(methods, [bridge::contract::METHOD_EVENTS]);
        assert_eq!(snapshot.auth_generation, 8);
        assert!(snapshot.account.is_none());
        assert!(
            snapshot.connect.is_none()
                && snapshot.server_trust.is_none()
                && snapshot.events.is_empty()
                && snapshot.last_disconnect == Some(1),
            "old Core route observations reached a replacement account selection"
        );
    })
    .await
    .expect("finite original Core response/selection fixture stalled");
}
