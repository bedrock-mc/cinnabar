use super::*;

const MAX_PROBE_REQUEST_BYTES: usize = 64 * 1024;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

/// Observes the original HTTPS attempt before serving the same address's actual HTTP probe.
async fn http_probe(listener: &TcpListener) -> TcpStream {
    let (mut tls, _) = listener.accept().await.unwrap();
    let mut first = [0];
    tls.read_exact(&mut first).await.unwrap();
    assert_eq!(first[0], 0x16, "HTTPS must precede HTTP discovery");
    drop(tls);
    let (mut http, _) = listener.accept().await.unwrap();
    let mut request = Vec::new();
    while !request.ends_with(b"\r\n\r\n") {
        assert!(request.len() < MAX_PROBE_REQUEST_BYTES);
        request.push(http.read_u8().await.unwrap());
    }
    let first = std::str::from_utf8(&request)
        .unwrap()
        .lines()
        .next()
        .unwrap();
    assert_eq!(
        first,
        format!(
            "GET {} HTTP/1.1",
            nethernet::signaling::http::join::STATUS_PATH
        )
    );
    http
}

/// A successful status cannot select HTTP before its original response body completes.
#[tokio::test]
async fn actual_probe_orders_https_before_http_and_waits_for_the_complete_body() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let deadline = tokio::time::Instant::now() + PROBE_TIMEOUT;
        let client = http::client(deadline).unwrap();
        let (sent, received) = tokio::sync::oneshot::channel();
        let (finish, ready) = tokio::sync::oneshot::channel();
        let server = async {
            let mut peer = http_probe(&listener).await;
            peer.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\na")
                .await.unwrap();
            sent.send(()).unwrap();
            ready.await.unwrap();
            peer.write_all(b"b").await.unwrap();
        };
        let selection = async {
            let mut selection = Box::pin(probe(&client, "127.0.0.1", Some(address.port()), deadline));
            tokio::select! { biased;
                result = selection.as_mut() => panic!("probe completed before the body barrier: {result:?}"),
                result = received => result.unwrap(),
            }
            assert!(futures::poll!(selection.as_mut()).is_pending());
            finish.send(()).unwrap();
            assert_eq!(selection.await, Some(format!("http://{address}")));
        };
        tokio::join!(server, selection);
    }).await.expect("actual ordered HTTP probe stalled");
}

/// A redirect never contacts its owned destination and never becomes a successful HTTP choice.
#[tokio::test]
async fn actual_probe_rejects_redirects_without_contacting_the_destination() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let redirect = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let destination = redirect.local_addr().unwrap();
        let deadline = tokio::time::Instant::now() + PROBE_TIMEOUT;
        let client = http::client(deadline).unwrap();
        let server = async {
            let mut peer = http_probe(&listener).await;
            peer.write_all(format!("HTTP/1.1 302 Found\r\nLocation: http://{destination}/must-not-follow\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
        };
        let selection = async {
            tokio::select! { biased;
                _ = redirect.accept() => panic!("native discovery followed a redirect"),
                result = probe(&client, "127.0.0.1", Some(address.port()), deadline) => assert_eq!(result, None),
            }
        };
        tokio::join!(server, selection);
    }).await.expect("actual redirect probe stalled");
}
