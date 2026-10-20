//! Owning control/session fixtures; no credentials, listener or account authority is fabricated.

use super::{control, registry::Registry};
use bridge::{
    AuthState,
    contract::*,
    relay_control::{Event, Hello},
};
use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use std::{
    net::{IpAddr, Ipv4Addr},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio_util::codec::Framed;

/// The real framed control EOF revokes its live play token before the account becomes detached.
#[tokio::test]
async fn control_eof_retires_the_original_play_controller() {
    let registry = Registry::new();
    let address = IpAddr::V4(Ipv4Addr::LOCALHOST);
    let (server, client) = tokio::io::duplex(RELAY_CONTROL_BYTES);
    let serving = control::run(&registry, address, server);
    let client = async {
        let mut client = Framed::new(client, bridge::BridgeCodec::with_max(RELAY_CONTROL_BYTES));
        client
            .send(Bytes::from(
                serde_json::to_vec(&Hello {
                    version: RELAY_VERSION,
                    channel: RELAY_CHANNEL_CONTROL.into(),
                    channels: vec![RELAY_CHANNEL_CONTROL.into(), RELAY_CHANNEL_PLAY.into()],
                    resume: String::new(),
                    generation: 0,
                    join: 0,
                })
                .unwrap(),
            ))
            .await
            .unwrap();
        let Event::Welcome { capability } =
            serde_json::from_slice(&client.next().await.unwrap().unwrap()).unwrap()
        else {
            panic!("actual control welcome")
        };
        let Event::Account { account } =
            serde_json::from_slice(&client.next().await.unwrap().unwrap()).unwrap()
        else {
            panic!("actual initial account")
        };
        assert_eq!(account.state, AuthState::SignedOut);
        let attachment = registry
            .open_play(&capability.resume, capability.generation, Instant::now())
            .unwrap();
        let lease = attachment
            .session
            .begin_play(capability.generation, 1)
            .await
            .unwrap();
        assert!(!lease.cancel.is_cancelled());
        client.close().await.unwrap();
        // This token is owned by the actual control handler; EOF, not a modeled flag, retires it.
        lease.cancel.cancelled().await;
        assert!(lease.current().is_err());
        drop(lease);
        drop(attachment);
    };
    let (result, ()) = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(serving, client)
    })
    .await
    .unwrap();
    assert!(result.is_err());
    registry.close();
}

/// A resumed controller cannot steal the previous socket's play lease before concrete cleanup ends.
#[tokio::test]
async fn newer_join_waits_for_the_actual_previous_play_owner() {
    let registry = Registry::new();
    let now = Instant::now();
    let first = registry
        .open_control(IpAddr::V4(Ipv4Addr::LOCALHOST), "", now)
        .unwrap();
    let original = first.session.begin_play(first.generation, 1).await.unwrap();
    assert!(first.session.begin_play(first.generation, 1).await.is_err());
    let newer = {
        let pending = first.session.begin_play(first.generation, 2);
        tokio::pin!(pending);
        assert!(futures::poll!(&mut pending).is_pending());
        assert!(original.current().is_ok());
        drop(original);
        pending.await.unwrap()
    };
    assert_eq!(newer.join, 2);
    assert!(newer.current().is_ok());
    let bearer = first.session.state.lock().unwrap().current.wire();
    let replacement = registry
        .open_control(IpAddr::V4(Ipv4Addr::LOCALHOST), &bearer, now)
        .unwrap();
    assert!(first.cancel.is_cancelled());
    drop(first);
    assert!(!replacement.cancel.is_cancelled());
    assert!(newer.current().is_err());
    let resumed = {
        let pending = replacement.session.begin_play(replacement.generation, 1);
        tokio::pin!(pending);
        assert!(futures::poll!(&mut pending).is_pending());
        drop(newer);
        pending.await.unwrap()
    };
    assert!(resumed.current().is_ok());
    drop(resumed);
    drop(replacement);
    registry.close();
}

/// Repeated takeover retains the originally presented capability deadline, without extending it.
#[test]
fn resume_preserves_original_expiry_and_replacement_controller() {
    let registry = Registry::new();
    let now = Instant::now();
    let address = IpAddr::V4(Ipv4Addr::LOCALHOST);
    let first = registry.open_control(address, "", now).unwrap();
    let original = first.session.state.lock().unwrap().current.wire();
    let resumed_at = now + Duration::from_secs(u64::from(RELAY_KEEPALIVE_SECONDS));
    let replacement = registry
        .open_control(address, &original, resumed_at)
        .unwrap();
    assert!(Arc::ptr_eq(&first.session, &replacement.session));
    let current = replacement.session.state.lock().unwrap().current.wire();
    drop(first);
    assert!(!replacement.cancel.is_cancelled());
    let after_original =
        now + Duration::from_secs(u64::from(RELAY_RESUME_SECONDS)) + Duration::from_nanos(1);
    assert!(
        registry
            .open_play(&original, replacement.generation, after_original)
            .is_err()
    );
    assert!(
        registry
            .open_play(&current, replacement.generation, after_original)
            .is_ok()
    );
    drop(replacement);
    registry.close();
}
