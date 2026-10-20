//! One counted QUIC connection and its sole WebTransport/control/account lifetime.

mod channels;

use super::{current, failed, origin};
use crate::{
    registry::{ConnectionPermit, Registry},
    session::closed,
};
use bridge::contract::*;
use bytes::Bytes;
use h3::ext::Protocol;
use h3_webtransport::server::{AcceptedBi, WebTransportSession};
use http::{Method, Request, Response, StatusCode};
use std::{
    io,
    net::IpAddr,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio_rustls::rustls;
use tokio_util::sync::CancellationToken;

pub(crate) type Transport = WebTransportSession<h3_quinn::Connection, Bytes>;
pub(super) type Bidi = h3_webtransport::stream::BidiStream<h3_quinn::BidiStream<Bytes>, Bytes>;

/// Applies the generated paired-listener limits to the same TLS certificate resolver.
pub(super) fn configuration(tls: Arc<rustls::ServerConfig>) -> io::Result<quinn::ServerConfig> {
    let crypto =
        quinn::crypto::rustls::QuicServerConfig::try_from((*tls).clone()).map_err(failed)?;
    let mut config = quinn::ServerConfig::with_crypto(Arc::new(crypto));
    let mut transport = quinn::TransportConfig::default();
    transport.max_concurrent_bidi_streams(
        u32::try_from(RELAY_QUIC_BIDIRECTIONAL_STREAMS)
            .map_err(failed)?
            .into(),
    );
    transport.max_concurrent_uni_streams(
        u32::try_from(RELAY_QUIC_UNIDIRECTIONAL_STREAMS)
            .map_err(failed)?
            .into(),
    );
    let window = u64::try_from(RELAY_PLAY_MESSAGE_BYTES).map_err(failed)?;
    transport.stream_receive_window(quinn::VarInt::from_u64(window).map_err(failed)?);
    transport.receive_window(
        quinn::VarInt::from_u64(window.checked_mul(2).ok_or_else(closed)?).map_err(failed)?,
    );
    let idle = Duration::from_secs(
        u64::from(RELAY_KEEPALIVE_SECONDS)
            .checked_mul(2)
            .ok_or_else(closed)?,
    );
    transport.max_idle_timeout(Some(idle.try_into().map_err(failed)?));
    config.transport_config(Arc::new(transport));
    Ok(config)
}

/// Retains admission before TLS and physical close through all stream/upstream retirement.
pub(super) async fn serve(
    registry: Arc<Registry>,
    origins: Arc<[String]>,
    peer: IpAddr,
    incoming: quinn::Incoming,
    permit: ConnectionPermit,
    accepted: Instant,
) -> io::Result<()> {
    let _permit = permit;
    let deadline = accepted
        .checked_add(Duration::from_secs(u64::from(RELAY_HELLO_SECONDS)))
        .ok_or_else(closed)?;
    current(deadline)?;
    let connecting = incoming.accept().map_err(failed)?;
    let connection = tokio::select! { biased;
        () = registry.cancelled.cancelled() => return Err(closed()),
        result = tokio::time::timeout_at(deadline.into(), connecting) => result.map_err(|_| timed_out())?.map_err(failed)?,
    };
    let _physical = Physical(connection.clone());
    current(deadline)?;
    let opening = open(connection.clone(), &origins, deadline);
    tokio::pin!(opening);
    let session = tokio::select! { biased;
        () = registry.cancelled.cancelled() => return Err(closed()),
        result = tokio::time::timeout_at(deadline.into(), &mut opening) => result.map_err(|_| timed_out())??,
    };
    let transport = Arc::new(session);
    let stop = CancellationToken::new();
    let result = {
        let serving = channels::run(
            &registry,
            peer,
            Arc::clone(&transport),
            deadline,
            stop.clone(),
        );
        let refusing_uni = refuse_uni(&transport);
        let closing = transport.recv_session_close();
        let maintaining = transport.maintain();
        tokio::pin!(serving, refusing_uni, closing, maintaining);
        tokio::select! { biased;
            result = &mut serving => result,
            () = registry.cancelled.cancelled() => {
                stop.cancel();
                serving.await
            }
            result = &mut closing => {
                stop.cancel();
                let cleanup = serving.await;
                result.map_err(failed).and(cleanup)
            }
            result = &mut refusing_uni => {
                stop.cancel();
                let cleanup = serving.await;
                result.and(cleanup)
            }
            result = &mut maintaining => {
                stop.cancel();
                let cleanup = serving.await;
                result.map_err(failed).and(cleanup)
            }
        }
    };
    // Every play/pack future and the borrowed CONNECT receiver has now released this same owner.
    let session = Arc::try_unwrap(transport).map_err(|_| failed(()))?;
    let close_deadline = Instant::now()
        .checked_add(Duration::from_secs(u64::from(RELAY_HELLO_SECONDS)))
        .ok_or_else(closed)?;
    let closing = session.close(0, "");
    let cleanup = match tokio::time::timeout_at(close_deadline.into(), closing).await {
        Ok(Ok(closed_session)) => {
            // The original h3 owner must outlive queued DATA/FIN; its Drop closes QUIC.
            let maintaining = closed_session.maintain();
            tokio::pin!(maintaining);
            tokio::select! { biased;
                _ = connection.closed() => Ok(()),
                result = &mut maintaining => result.map_err(failed),
                () = tokio::time::sleep_until(close_deadline.into()) => Err(timed_out()),
            }
        }
        Ok(Err(error)) => Err(failed(error)),
        Err(_) => Err(timed_out()),
    };
    result.and(cleanup)
}

/// Validates the maintained decoder's actual request before any successful CONNECT response.
async fn open(
    connection: quinn::Connection,
    origins: &[String],
    deadline: Instant,
) -> io::Result<Transport> {
    let mut builder = h3::server::builder();
    let headers = u64::try_from(RELAY_CONTROL_BYTES).map_err(failed)?;
    builder
        .enable_webtransport(true)
        .enable_extended_connect(true)
        .enable_datagram(true)
        .max_webtransport_sessions(1)
        .max_field_section_size(headers)
        .max_encoded_headers_size(headers);
    let physical = connection.clone();
    let mut connection = builder
        .build(h3_quinn::Connection::new(connection))
        .await
        .map_err(failed)?;
    let resolver = connection
        .accept()
        .await
        .map_err(failed)?
        .ok_or_else(closed)?;
    let (request, mut response) = resolver.resolve_request().await.map_err(failed)?;
    current(deadline)?;
    if let Err(status) = inspect(&request, origins) {
        response
            .send_response(
                Response::builder()
                    .status(status)
                    .body(())
                    .map_err(failed)?,
            )
            .await
            .map_err(failed)?;
        response.finish().await.map_err(failed)?;
        // Dropping the original h3 connection closes QUIC, which can discard a queued refusal.
        // Keep it through actual peer close or the original accepted-time budget.
        tokio::select! { biased;
            _ = physical.closed() => {},
            () = tokio::time::sleep_until(deadline.into()) => {},
        }
        return Err(failed(()));
    }
    WebTransportSession::accept(request, response, connection)
        .await
        .map_err(failed)
}

/// The first Origin retains Go Header.Get semantics; neither URI nor origin authorizes an upstream.
fn inspect(request: &Request<()>, origins: &[String]) -> Result<(), StatusCode> {
    if request.method() != Method::CONNECT
        || request.extensions().get::<Protocol>() != Some(&Protocol::WEB_TRANSPORT)
        || request.uri().scheme_str() != Some("https")
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let authority = request
        .uri()
        .authority()
        .filter(|authority| !authority.as_str().is_empty())
        .ok_or(StatusCode::BAD_REQUEST)?;
    if request.uri().path() != RELAY_TRANSPORT_PATH {
        return Err(StatusCode::NOT_FOUND);
    }
    let origin = request
        .headers()
        .get(http::header::ORIGIN)
        .map(|value| value.to_str())
        .transpose()
        .map_err(|_| StatusCode::BAD_REQUEST)?
        .unwrap_or_default();
    if !origin::allowed(origin, authority.as_str(), origins) {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(())
}

/// Owns the taken stream and its partial protocol prefix across unrelated select completions.
async fn next_bi(transport: Arc<Transport>) -> io::Result<Bidi> {
    loop {
        let accepted = transport
            .accept_bi()
            .await
            .map_err(failed)?
            .ok_or_else(closed)?;
        match accepted {
            AcceptedBi::BidiStream(session, mut stream) => {
                if session == transport.session_id() {
                    return Ok(stream);
                }
                use h3::quic::{RecvStream, SendStream};
                stream.stop_sending(0);
                stream.reset(0);
            }
            AcceptedBi::Request(_, mut stream) => {
                // The advertised maximum is not an admission check; no second session is built.
                stream
                    .send_response(
                        Response::builder()
                            .status(StatusCode::TOO_MANY_REQUESTS)
                            .body(())
                            .map_err(failed)?,
                    )
                    .await
                    .map_err(failed)?;
                stream.stop_sending(h3::error::Code::H3_REQUEST_REJECTED);
                stream.finish().await.map_err(failed)?;
            }
        }
    }
}

/// Lets the original HTTP/3 driver consume control/QPACK while rejecting all client application uni.
async fn refuse_uni(transport: &Transport) -> io::Result<()> {
    use h3::quic::RecvStream;
    loop {
        let (_, mut stream) = transport
            .accept_uni()
            .await
            .map_err(failed)?
            .ok_or_else(closed)?;
        stream.stop_sending(0);
    }
}

/// QUIC closes only after the original application handlers have retired, including on future Drop.
struct Physical(quinn::Connection);
impl Drop for Physical {
    fn drop(&mut self) {
        self.0.close(0_u32.into(), b"");
    }
}

/// Reports only expiry of the original finite handshake or close budget.
fn timed_out() -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, "gateway WebTransport timed out")
}

#[cfg(test)]
pub(crate) mod tests;
