//! One retained product socket on the existing application executor, with no background reader task.

use super::*;
use crate::native_account::{ApiCredential, ApiScope, NativeAccount};
use bridge::contract::CINNABAR_API_MAX_FRAME_BYTES;
use futures::{SinkExt, StreamExt};
use std::{io, time::Duration};
use tokio::time::Instant as Deadline;
use tokio_websockets::{ClientBuilder, Config, Limits, Message};

struct Failure {
    stage: &'static str,
    code: u16,
    minimum: Duration,
}

impl Failure {
    /// Keeps errors redacted while retaining retry-relevant stage and close status.
    fn at(stage: &'static str) -> Self {
        Self {
            stage,
            code: 0,
            minimum: Duration::ZERO,
        }
    }

    /// Reads only explicit HTTP429 retry metadata from the original parser, without retaining headers.
    fn dial(error: tokio_websockets::Error) -> Self {
        let minimum = error
            .upgrade_response()
            .filter(|response| response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS)
            .and_then(|response| response.headers().get("retry-after"))
            .and_then(|value| value.to_str().ok())
            .map(|value| retry_after(value, SystemTime::now()))
            .unwrap_or_default();
        Self {
            stage: "dial",
            code: 0,
            minimum,
        }
    }
}

impl Client {
    /// Owns only this optional product service; None remains offline without authenticating or using Core.
    pub(crate) async fn run(&self, account: Option<Arc<NativeAccount>>) {
        let Some(account) = account else {
            self.stop();
            return;
        };
        let base = CINNABAR_API_BASE_URL
            .trim_end_matches('/')
            .replacen("https://", "wss://", 1);
        self.run_at(account, &format!("{base}/v1/connect")).await;
        self.stop();
    }

    /// Drives the same original socket owner against the fixed production URL or a private loopback test.
    pub(super) async fn run_at(&self, account: Arc<NativeAccount>, url: &str) {
        let mut credential = None;
        let mut scope = None;
        let mut failures = 0u32;
        let mut rejections = 0u32;
        loop {
            let changed = self.0.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            let (stopped, epoch, config) = self.with(|s| (s.stopped, s.epoch, s.config.clone()));
            if stopped {
                return;
            }
            let Some(config) = config else {
                changed.await;
                continue;
            };
            let started = Deadline::now();
            let result = tokio::select! { biased;
                _=self.superseded(epoch)=>continue,
                result=self.connection(&account,url,epoch,&config,&mut scope,&mut credential)=>result,
            };
            self.with(|state| {
                if state.epoch == epoch {
                    state.live = None;
                }
            });
            if !self.current(epoch) {
                continue;
            }
            if scope
                .as_ref()
                .is_some_and(|scope: &ApiScope| scope.current() == Some(false))
            {
                return;
            }
            let error = match result {
                Ok(()) => Failure::at("session"),
                Err(error) => error,
            };
            if error.stage == "session" && started.elapsed() >= Duration::from_secs(60) {
                failures = 0;
            }
            failures = failures.saturating_add(1);
            if error.code == 4401 {
                credential = None;
            }
            if matches!(error.code, 1007..=1009) {
                rejections += 1;
                if rejections >= 3 {
                    return;
                }
            }
            let cap = self.with(|state| state.parameters.reconnect);
            let base = policy::backoff(failures, cap, rand::random());
            let jitter = if error.minimum > Duration::ZERO
                || matches!(error.stage, "session" | "handshake")
                || error.code >= 1000
            {
                Duration::from_nanos(
                    rand::random::<u64>()
                        % (u64::try_from(policy::RECONNECT_SPREAD.as_nanos()).unwrap() + 1),
                )
            } else {
                Duration::ZERO
            };
            let delay = base.max(error.minimum).saturating_add(jitter);
            tracing::debug!(
                stage = error.stage,
                close_code = error.code,
                "Cinnabar product API unavailable; retrying"
            );
            tokio::select! { biased;
                _=self.superseded(epoch)=>{},
                _=tokio::time::sleep(delay)=>{},
            }
        }
    }

    /// Dials before minting under the one original ten-second authentication deadline.
    async fn connection(
        &self,
        account: &Arc<NativeAccount>,
        url: &str,
        epoch: u64,
        config: &CinnabarApiConfig,
        scope: &mut Option<ApiScope>,
        credential: &mut Option<ApiCredential>,
    ) -> Result<(), Failure> {
        let auth_until = Deadline::now() + REQUEST_TIMEOUT;
        let auth = async {
            let tls = tokio_rustls::rustls::ClientConfig::builder_with_provider(Arc::new(
                tokio_rustls::rustls::crypto::aws_lc_rs::default_provider(),
            ))
            .with_safe_default_protocol_versions()
            .map_err(|_| Failure::at("dial"))?
            .with_root_certificates(tokio_rustls::rustls::RootCertStore::from_iter(
                webpki_roots::TLS_SERVER_ROOTS.iter().cloned(),
            ))
            .with_no_client_auth();
            let connector = tokio_websockets::Connector::Rustls(Arc::new(tls).into());
            let builder = ClientBuilder::new()
                .uri(url)
                .map_err(|_| Failure::at("dial"))?
                .connector(&connector)
                .add_header(
                    reqwest::header::USER_AGENT,
                    reqwest::header::HeaderValue::from_str(&format!(
                        "cinnabar/{}",
                        config.client_version
                    ))
                    .map_err(|_| Failure::at("dial"))?,
                )
                .map_err(|_| Failure::at("dial"))?
                .capture_rejected_response(true)
                .config(
                    Config::default()
                        .frame_size(CINNABAR_API_MAX_FRAME_BYTES)
                        .flush_threshold(1),
                )
                .limits(Limits::default().max_payload_len(Some(CINNABAR_API_MAX_FRAME_BYTES)));
            let (mut socket, response) = builder.connect().await.map_err(Failure::dial)?;
            if Deadline::now() >= auth_until {
                return Err(Failure::at("token"));
            }
            if response.headers().contains_key("sec-websocket-extensions")
                || response.headers().contains_key("sec-websocket-protocol")
            {
                return Err(Failure::at("dial"));
            }
            if !credential
                .as_ref()
                .is_some_and(|credential| credential.reusable(Deadline::now()))
            {
                *credential = Some(
                    Arc::clone(account)
                        .product_api_credential(scope.as_ref())
                        .await
                        .map_err(|_| Failure::at("token"))?,
                );
            }
            let credential = credential.as_ref().expect("minted product credential");
            *scope = Some(credential.scope.clone());
            authority(&credential.scope, auth_until)
                .await
                .map_err(|_| Failure::at("token"))?;
            if !self.with(|state| {
                if state.stopped || state.epoch != epoch {
                    return false;
                }
                state.scope = Some(credential.scope.clone());
                true
            }) {
                return Err(Failure::at("handshake"));
            }
            let mut hello = json!({"type":"hello","protocol":1,"client_version":config.client_version,"channel":config.channel,"visible":config.visible,"token":credential.token()});
            if config.client_build != 0 {
                hello["client_build"] = json!(config.client_build);
            }
            socket
                .send(Message::text(hello.to_string()))
                .await
                .map_err(|_| Failure::at("handshake"))?;
            loop {
                let message = socket
                    .next()
                    .await
                    .ok_or_else(|| Failure::at("handshake"))?
                    .map_err(|_| Failure::at("handshake"))?;
                if let Some((code, reason)) = message.as_close() {
                    return Err(closed("handshake", code.into(), reason));
                }
                let Some(message) = message.as_text().and_then(Inbound::parse) else {
                    continue;
                };
                authority(&credential.scope, auth_until)
                    .await
                    .map_err(|_| Failure::at("handshake"))?;
                match message.kind.as_str() {
                    "challenge" => {
                        let signature = credential
                            .proof(&message.nonce)
                            .map_err(|_| Failure::at("handshake"))?;
                        socket
                            .send(Message::text(
                                json!({"type":"proof","signature":signature}).to_string(),
                            ))
                            .await
                            .map_err(|_| Failure::at("handshake"))?;
                    }
                    "welcome" => {
                        if message.xuid != credential.scope.xuid()
                            || !self.welcome(epoch, config, credential.scope.clone(), message)
                        {
                            return Err(Failure::at("handshake"));
                        }
                        return Ok(socket);
                    }
                    "error" => {
                        return Err(Failure {
                            stage: "handshake",
                            code: 4401,
                            minimum: Duration::ZERO,
                        });
                    }
                    "release" => self.receive(epoch, message, Instant::now()),
                    _ => {}
                }
            }
        };
        let mut socket = bounded(auth_until, auth)
            .await
            .map_err(|_| Failure::at("handshake"))??;
        let scope = scope.as_ref().expect("authenticated original scope");
        let p = self.with(|state| state.parameters);
        let idle = p.ping.saturating_mul(3).max(
            Duration::from_millis(u64::from(CINNABAR_API_PING_INTERVAL_MS) * 2)
                + Duration::from_secs(15),
        );
        let mut read_until = Deadline::now() + idle;
        let mut ping_at = Deadline::now() + p.ping;
        loop {
            let changed = self.0.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            authority(scope, read_until)
                .await
                .map_err(|_| Failure::at("session"))?;
            if let Some((payload, until)) = self.next_payload(epoch, Instant::now()) {
                let send = socket.send(Message::text(payload.to_string()));
                tokio::select! { biased;
                    _=retired(scope)=>return Err(Failure::at("session")),
                    result=bounded(Deadline::from_std(until),send)=>result.map_err(|_| Failure::at("session"))?.map_err(|_| Failure::at("session"))?,
                }
                continue;
            }
            tokio::select! { biased;
                _=retired(scope)=>return Err(Failure::at("session")),
                message=bounded(read_until,socket.next())=>{
                    let message=message.map_err(|_| Failure::at("session"))?.ok_or_else(|| Failure::at("session"))?.map_err(|_| Failure::at("session"))?;
                    authority(scope,read_until).await.map_err(|_| Failure::at("session"))?;
                    read_until=Deadline::now()+idle;
                    if let Some((code,reason))=message.as_close() { return Err(closed("session",code.into(),reason)); }
                    if let Some(message)=message.as_text().and_then(Inbound::parse) { self.receive(epoch,message,Instant::now()); }
                }
                _=tokio::time::sleep_until(ping_at)=>{
                    tokio::select! { biased;
                        _=retired(scope)=>return Err(Failure::at("session")),
                        result=bounded(Deadline::now()+REQUEST_TIMEOUT,socket.send(Message::text("ping")))=>result.map_err(|_| Failure::at("session"))?.map_err(|_| Failure::at("session"))?,
                    }
                    ping_at=Deadline::now()+p.ping;
                }
                _=changed=>{},
            }
        }
    }
}

/// Retains a completed result through Busy under its original absolute cutoff.
async fn authority(scope: &ApiScope, until: Deadline) -> io::Result<()> {
    bounded(until, async {
        loop {
            match scope.current() {
                Some(true) => return Ok(()),
                Some(false) => {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "product API account retired",
                    ));
                }
                None => tokio::time::sleep(crate::native_account::LEASE_RETRY).await,
            }
        }
    })
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "product API identity deadline"))?
}

/// Checks the original cutoff before polling even ready I/O, retaining the same future across wakes.
pub(super) async fn bounded<T>(
    until: Deadline,
    future: impl std::future::Future<Output = T>,
) -> Result<T, ()> {
    use std::future::{Future, poll_fn};
    let timer = tokio::time::sleep_until(until);
    tokio::pin!(timer, future);
    poll_fn(|cx| {
        if Deadline::now() >= until || timer.as_mut().poll(cx).is_ready() {
            return std::task::Poll::Ready(Err(()));
        }
        future.as_mut().poll(cx).map(Ok)
    })
    .await
}

/// Busy never signals revocation and never replaces the original in-flight socket future.
async fn retired(scope: &ApiScope) {
    loop {
        if scope.current() == Some(false) {
            return;
        }
        tokio::time::sleep(crate::native_account::LEASE_RETRY).await;
    }
}

/// Retains only the close status and explicit server retry floor, never raw close text in diagnostics.
fn closed(stage: &'static str, code: u16, reason: &str) -> Failure {
    let minimum = if code == 4429 {
        retry_after(reason, SystemTime::now())
    } else {
        Duration::ZERO
    };
    Failure {
        stage,
        code,
        minimum,
    }
}

/// Honors the original server floor for delta seconds or HTTP dates, with room for retry jitter.
fn retry_after(value: &str, now: SystemTime) -> Duration {
    let value = value
        .trim()
        .strip_prefix("retry_after=")
        .unwrap_or(value.trim());
    let limit = Duration::from_nanos(i64::MAX as u64) - policy::RECONNECT_SPREAD;
    if let Ok(seconds) = value.parse::<u64>() {
        return Duration::from_secs(seconds).min(limit);
    }
    httpdate::parse_http_date(value)
        .ok()
        .and_then(|at| at.duration_since(now).ok())
        .unwrap_or_default()
        .min(limit)
}
