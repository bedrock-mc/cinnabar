//! A single actual full-ICE HTTP answer, with only its candidates removed by a loopback peer.

use super::*;
use crate::login::native_network::transport::early_spawn_tests::{Route, fixture_route};
use nethernet::signaling::http::join;
use p384::{ecdsa::SigningKey, elliptic_curve::Generate};
use std::time::SystemTime;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

/// Local HTTP must retain its specific no-route cause; a bad signature keeps its ordinary error.
#[tokio::test]
async fn local_http_candidateless_answer_reports_offline_without_hiding_invalid_identity() {
    tokio::time::timeout(crate::NATIVE_JOIN_TIMEOUT * 2, async {
        for corrupt_identity in [false, true] {
            let mut route = fixture_route(Route::LocalHttp).await;
            let proxy = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = proxy.local_addr().unwrap();
            let original = route.address;
            let deadline = tokio::time::Instant::now() + crate::NATIVE_JOIN_TIMEOUT / 4;
            let service = tokio::spawn(async move {
                let (mut tcp, _) = proxy.accept().await.unwrap();
                let mut head = Vec::new();
                while !head.ends_with(b"\r\n\r\n") {
                    assert!(head.len() < join::MAX_ANSWER_SIZE);
                    head.push(tcp.read_u8().await.unwrap());
                }
                let head = std::str::from_utf8(&head).unwrap();
                let mut request = head.lines().next().unwrap().split_whitespace();
                assert_eq!(request.next(), Some("POST"));
                let path = request.next().unwrap();
                let length = head
                    .lines()
                    .filter_map(|line| line.split_once(':'))
                    .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                    .unwrap()
                    .1
                    .trim()
                    .parse::<usize>()
                    .unwrap();
                assert!(length <= join::MAX_ANSWER_SIZE);
                let mut offer = vec![0; length];
                tcp.read_exact(&mut offer).await.unwrap();
                let answer = client(deadline)
                    .unwrap()
                    .post(format!("http://{original}{path}"))
                    .header(reqwest::header::CONTENT_TYPE, join::CONTENT_TYPE)
                    .header(reqwest::header::USER_AGENT, join::CLIENT_USER_AGENT)
                    .body(offer)
                    .send()
                    .await
                    .unwrap()
                    .error_for_status()
                    .unwrap()
                    .text()
                    .await
                    .unwrap();
                assert!(answer.lines().any(|line| line.starts_with("a=candidate:")));
                let mut answer = answer
                    .lines()
                    .filter(|line| !line.starts_with("a=candidate:"))
                    .map(|line| format!("{line}\r\n"))
                    .collect::<String>();
                if corrupt_identity {
                    assert!(answer.contains("a=identity:"));
                    answer = answer.replace("a=identity:", "a=identity:invalid");
                }
                tcp.write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        answer.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
                tcp.write_all(answer.as_bytes()).await.unwrap();
                tcp.shutdown().await.unwrap();
            });
            let identity = TransportIdentity {
                identity: Arc::new(
                    nethernet::identity::SelfSignedIdentity::from_key(
                        Arc::new(SigningKey::generate()),
                        SystemTime::now(),
                        crate::NATIVE_JOIN_TIMEOUT,
                    )
                    .unwrap(),
                ),
                trust: crate::login::native_network::authentication::offline_answer_trust(),
                expires: None,
            };
            let result = connect(address, identity, deadline).await;
            service.await.unwrap();
            route.listener.close().await.unwrap();
            drop(route.listener);
            Arc::try_unwrap(route.http.unwrap())
                .ok()
                .unwrap()
                .shutdown()
                .await;
            let error = result
                .err()
                .expect("an answer without any route must not become Ready");
            if corrupt_identity {
                assert_ne!(
                    error.to_string(),
                    bridge::contract::DISCONNECT_KEY_LOCAL_WORLD_OFFLINE
                );
            } else {
                assert_eq!(error.kind(), io::ErrorKind::NotConnected);
                assert_eq!(
                    error.to_string(),
                    bridge::contract::DISCONNECT_KEY_LOCAL_WORLD_OFFLINE
                );
            }
        }
    })
    .await
    .expect("finite original local HTTP no-route control stalled");
}
