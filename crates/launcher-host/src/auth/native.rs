//! Native acquisition on a background owner; the menu receives only presentation and verified metadata.

use bedrock_services::{
    oauth::{Credentials, MicrosoftAuth, Poll},
    xbox,
};
use crossbeam_channel::{Receiver, Sender, bounded};
use launcher::{
    accounts::{AccountProfile, AccountStore, pending::PendingSignIn},
    menu::auth::AuthState,
};
use std::{
    io,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime},
};
use tokio::sync::oneshot;

use crate::native_account::{
    ACQUISITION_TIMEOUT, LEASE_RETRY, NativeAccount, REFRESH_ONLY_EXPIRY, XBOX_RELYING_PARTY,
    authentication_policy, oauth_expiry, requires_sign_in,
};

const SIGN_IN_DEADLINE: Duration = Duration::from_secs(20 * 60);
const PUBLICATION_DEADLINE: Duration = Duration::from_secs(3);

mod signup;
use signup::Signup;

enum Message {
    State(AuthState),
    Verified(AccountProfile, Option<PendingSignIn>),
}

/// Owns cancellation and bounded presentation delivery, without exposing SDK secrets in Debug.
pub(super) struct NativeAuth {
    receiver: Receiver<Message>,
    cancel: Option<oneshot::Sender<()>>,
    done: Arc<AtomicBool>,
    state: AuthState,
    profile: Option<(AccountProfile, Option<PendingSignIn>)>,
    cancelled: bool,
    generation: String,
}

impl std::fmt::Debug for NativeAuth {
    /// Excludes browser URLs, account metadata and credentials from diagnostics.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NativeAuth")
    }
}

struct Completion(Arc<AtomicBool>);
impl Drop for Completion {
    /// Makes cleanup observable after HTTP cancellation and bounded persistence finish.
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

impl NativeAuth {
    /// Supplies a completed deterministic presenter fixture without threads, HTTP or credentials.
    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn from_state(state: AuthState) -> Self {
        let (_, receiver) = bounded(super::EVENT_CAPACITY);
        Self {
            receiver,
            cancel: None,
            done: Arc::new(AtomicBool::new(true)),
            state,
            profile: None,
            cancelled: false,
            generation: "synthetic-presenter".into(),
        }
    }

    /// Starts the application's existing background-thread/current-thread-runtime pattern.
    pub(super) fn spawn(
        store: AccountStore,
        device: PathBuf,
        selected: Option<String>,
        adding: bool,
        runtime: tokio::runtime::Handle,
        tasks: tokio_util::task::TaskTracker,
    ) -> io::Result<Self> {
        let (sender, receiver) = bounded(super::EVENT_CAPACITY);
        let (cancel, cancelled) = oneshot::channel();
        let done = Arc::new(AtomicBool::new(false));
        let completion = Completion(Arc::clone(&done));
        let generation = uuid::Uuid::new_v4().to_string();
        let worker_generation = generation.clone();
        std::thread::Builder::new()
            .name("native-sign-in".into())
            .spawn(crate::worker::tracked(
                move || {
                    let _completion = completion;
                    let result = runtime.block_on(run(
                        store,
                        device,
                        selected,
                        adding,
                        worker_generation,
                        &sender,
                        cancelled,
                    ));
                    if let Err(error) = result {
                        let state = match error {
                            Failure::Cancelled => AuthState::SignedOut,
                            Failure::Failed(message) => {
                                tracing::warn!(reason = message, "native sign-in failed");
                                AuthState::Failed(message.into())
                            }
                        };
                        let _ = sender.try_send(Message::State(state));
                    }
                },
                &tasks,
            ))?;
        Ok(Self {
            receiver,
            cancel: Some(cancel),
            done,
            state: AuthState::Checking,
            profile: None,
            cancelled: false,
            generation,
        })
    }

    /// Drains only bounded, nonsensitive worker messages on the frame.
    pub(super) fn poll(&mut self) {
        for _ in 0..super::EVENT_CAPACITY {
            match self.receiver.try_recv() {
                Ok(Message::State(state)) if !self.cancelled => self.state = state,
                Ok(Message::Verified(profile, pending)) if !self.cancelled => {
                    self.profile = Some((profile, pending));
                    self.state = AuthState::Authenticated;
                }
                Ok(_) => {}
                Err(crossbeam_channel::TryRecvError::Empty) => break,
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    if matches!(
                        self.state,
                        AuthState::Checking
                            | AuthState::AwaitingCode { .. }
                            | AuthState::AwaitingXboxSignup { .. }
                    ) {
                        self.state = AuthState::Failed(
                            "Sign-in stopped before it completed. Try again.".into(),
                        );
                    }
                    break;
                }
            }
        }
    }

    /// Cancels HTTP promptly; the worker then persists any SDK-visible rotation before reporting cleanup.
    pub(super) fn cancel(&mut self, dismiss_error: bool) {
        self.cancelled = true;
        self.profile = None;
        if let Some(cancel) = self.cancel.take() {
            let _ = cancel.send(());
        }
        if (dismiss_error && matches!(self.state, AuthState::Failed(_)))
            || !matches!(self.state, AuthState::Authenticated | AuthState::Failed(_))
        {
            self.state = AuthState::SignedOut;
        }
    }

    /// Exposes only the existing UI status, never the pending device credential.
    pub(super) fn state(&self) -> &AuthState {
        &self.state
    }
    /// Reports completion after all bounded persistence, without joining a thread on the frame.
    pub(super) fn complete(&self) -> bool {
        self.done.load(Ordering::Acquire)
    }
    /// Identifies this attempt without exposing its device or OAuth credential.
    pub(super) fn generation(&self) -> &str {
        &self.generation
    }
    /// Moves verified metadata and its optional pending transaction exactly once to the account owner.
    pub(super) fn take_profile(&mut self) -> Option<(AccountProfile, Option<PendingSignIn>)> {
        self.profile.take()
    }
}

impl Drop for NativeAuth {
    /// Detaches cancellation; no frame waits on network or filesystem work.
    fn drop(&mut self) {
        self.cancel(false);
    }
}

enum Failure {
    Cancelled,
    Failed(&'static str),
}

/// Keeps a concrete attempt/account outside cancellable HTTP futures so acquired refresh ownership survives.
async fn run(
    store: AccountStore,
    device: PathBuf,
    selected: Option<String>,
    adding: bool,
    generation: String,
    sender: &Sender<Message>,
    mut cancel: oneshot::Receiver<()>,
) -> Result<(), Failure> {
    if !adding {
        let account = Arc::new(NativeAccount::new(store.clone(), device, selected.clone()));
        if let Some(profile) =
            signup::saved_profile(&store, account, selected.as_deref(), sender, &mut cancel).await?
        {
            return send(sender, Message::Verified(profile, None));
        }
    }
    let pending = tokio::select! {
        biased;
        _ = &mut cancel => return Err(Failure::Cancelled),
        result = retry(|| store.try_begin_native_sign_in(&generation, if adding { None } else { selected.as_deref() })) =>
            result.map_err(|_| Failure::Failed("The account changed or is busy. Try again."))?,
    };
    let mut auth = MicrosoftAuth::new(authentication_policy())
        .map_err(|_| Failure::Failed("Sign-in could not start. Try again."))?;
    let credentials = tokio::select! {
        biased;
        _ = &mut cancel => return Err(Failure::Cancelled),
        result = tokio::time::timeout(SIGN_IN_DEADLINE, device_credentials(&mut auth, sender)) =>
            result.map_err(|_| Failure::Failed("Your sign-in code expired. Try again."))??,
    };
    // Microsoft ownership is durable before constructing a proof owner or issuing any Xbox request.
    persist(&pending, &credentials).await?;
    let mut account = auth
        .into_account(credentials)
        .map_err(|_| Failure::Failed("Could not create the account owner. Try again."))?;
    persist_proof(&pending, &account).await?;
    let authorization = authorize_pending(&mut account, &pending, sender, &mut cancel).await?;
    let profile = AccountProfile::from_identity(authorization.xuid(), authorization.gamertag())
        .map_err(|_| {
            Failure::Failed("Xbox returned invalid account profile metadata. Try again.")
        })?;
    if !adding && selected.as_deref().is_some_and(|id| id != profile.id) {
        return Err(Failure::Failed(
            "Xbox sign-in did not match the selected account. Try again.",
        ));
    }
    // The menu's existing mutation worker stops the previous core before promoting pending state.
    send(sender, Message::Verified(profile, Some(pending)))
}

/// Cancels only the borrowed HTTP future, then durably retains the concrete account's latest credential.
async fn authorize_pending(
    account: &mut xbox::Account,
    pending: &PendingSignIn,
    sender: &Sender<Message>,
    cancel: &mut oneshot::Receiver<()>,
) -> Result<Arc<xbox::Authorization>, Failure> {
    let mut signup = Signup::default();
    loop {
        let cutoff = signup.acquisition_cutoff(tokio::time::Instant::now())?;
        let result = tokio::select! {
            biased;
            _ = &mut *cancel => Err(Failure::Cancelled),
            result = tokio::time::timeout_at(cutoff, account.authorize(XBOX_RELYING_PARTY)) =>
                result.map_err(|_| Failure::Failed("Xbox sign-in timed out. Try again.")),
        };
        // Every borrowed attempt publishes SDK-visible rotation before presenting or retrying.
        persist(pending, account.credentials()).await?;
        persist_proof(pending, account).await?;
        match result? {
            Ok(authorization) => return Ok(authorization),
            Err(bedrock_services::Error::AccountCreationRequired(challenge)) => {
                signup.present(challenge.signup_url(), tokio::time::Instant::now(), sender)?;
                signup.wait(cancel).await?;
            }
            Err(_) => {
                return Err(Failure::Failed(
                    "Could not verify the Xbox profile. Try again.",
                ));
            }
        }
    }
}

/// Uses the SDK's explicit pacing and slow-down state without creating a hidden polling task.
async fn device_credentials(
    auth: &mut MicrosoftAuth,
    sender: &Sender<Message>,
) -> Result<Credentials, Failure> {
    let mut attempt = auth
        .start(SystemTime::now())
        .await
        .map_err(device_failure)?;
    let uri = attempt.verification_uri().as_str();
    if !super::valid_https_uri(uri)
        || uri.len() > super::MAX_LINE_BYTES
        || !super::valid_code(attempt.user_code())
    {
        return Err(Failure::Failed(
            "Microsoft returned an invalid sign-in prompt. Try again.",
        ));
    }
    send(
        sender,
        Message::State(AuthState::AwaitingCode {
            uri: uri.into(),
            code: attempt.user_code().into(),
        }),
    )?;
    loop {
        tokio::time::sleep(
            attempt
                .next_poll()
                .duration_since(SystemTime::now())
                .unwrap_or_default(),
        )
        .await;
        match auth
            .poll(&mut attempt, SystemTime::now())
            .await
            .map_err(device_failure)?
        {
            Poll::Pending => {}
            Poll::Authorized(credentials) => return Ok(credentials),
        }
    }
}

/// Gives protected disk publication a short independent grace even after network cancellation.
async fn persist(pending: &PendingSignIn, credentials: &Credentials) -> Result<(), Failure> {
    let expiry = oauth_expiry(credentials.expires_at());
    let result = match expiry {
        Ok(expiry) => {
            retry(|| {
                pending.try_publish(
                    credentials.access_token(),
                    credentials.refresh_token(),
                    &expiry,
                )
            })
            .await
        }
        Err(error) => Err(error),
    };
    if let Err(error) = result {
        if matches!(
            error.kind(),
            io::ErrorKind::FileTooLarge | io::ErrorKind::InvalidInput
        ) {
            retry(|| pending.try_publish("", credentials.refresh_token(), REFRESH_ONLY_EXPIRY))
                .await
                .map_err(|_| {
                    Failure::Failed("Could not save the sign-in credential. Try again.")
                })?;
        }
        return Err(Failure::Failed(
            "Could not save the sign-in credential. Try again.",
        ));
    }
    Ok(())
}

/// Encodes proof after OAuth durability, using the same pending-attempt fence.
async fn persist_proof(pending: &PendingSignIn, account: &xbox::Account) -> Result<(), Failure> {
    let checkpoint = account
        .checkpoint()
        .map_err(|_| Failure::Failed("Could not save the sign-in proof. Try again."))?;
    retry(|| pending.try_publish_checkpoint(checkpoint.as_bytes()))
        .await
        .map_err(|_| Failure::Failed("Could not save the sign-in proof. Try again."))
}

/// Retries only contention within a bounded edge deadline; stale generations and I/O errors fail immediately.
async fn retry<T>(mut operation: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    tokio::time::timeout(PUBLICATION_DEADLINE, async {
        loop {
            match operation() {
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    tokio::time::sleep(LEASE_RETRY).await
                }
                result => return result,
            }
        }
    })
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "account publication is busy"))?
}

/// Converts only known OAuth outcomes to controlled presenter copy.
fn device_failure(error: bedrock_services::Error) -> Failure {
    Failure::Failed(match error {
        bedrock_services::Error::DeviceCodeExpired => "Your sign-in code expired. Try again.",
        bedrock_services::Error::AuthorizationDeclined => {
            "Microsoft sign-in was declined. Try again."
        }
        _ => "Microsoft sign-in did not complete. Try again.",
    })
}

/// Bounded delivery fails closed when a dismissed presenter no longer accepts events.
fn send(sender: &Sender<Message>, message: Message) -> Result<(), Failure> {
    sender.try_send(message).map_err(|_| Failure::Cancelled)
}

#[cfg(test)]
mod tests;
