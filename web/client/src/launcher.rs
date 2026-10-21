//! Browser account presentation and addressed-server admission for the shared game menu.

pub mod preferences;
#[cfg(test)]
mod tests;

use ::launcher::menu::{
    MAX_SERVER_ADDRESS_BYTES,
    auth::{AccountIntent, AuthState},
    view::{JoinStage, MenuProfile},
};
use bridge::{Account, contract::ConnectTarget};
use web_transport::session::{AccountOperation, ControlEvent};

/// Distinguishes a lost connection from an operation the live relay refused.
#[derive(Clone, Copy)]
enum Failure {
    Disconnected,
    Refused(&'static str),
}

impl Failure {
    /// Supplies a bounded user message without displaying relay-provided text.
    fn message(self) -> &'static str {
        match self {
            Self::Disconnected => "The relay disconnected. Reconnect to continue.",
            Self::Refused(message) => message,
        }
    }
}

/// A browser-local view of one relay account; the Session owns commands and reconnects.
#[derive(Clone, Default)]
pub struct RelayAccount {
    account: Option<Account>,
    pending: Option<AccountOperation>,
    /// The Session or relay may retain this attempt until cancellation or completed sign-in.
    cancel_needed: bool,
    dismissed: bool,
    failure: Option<Failure>,
    progress: Option<JoinStage>,
}

impl RelayAccount {
    /// Replaces the outstanding account intent without an unbounded UI queue.
    pub fn request(&mut self, intent: AccountIntent) {
        self.failure = None;
        match intent {
            AccountIntent::SignIn => {
                if self
                    .account
                    .as_ref()
                    .is_some_and(|account| account.state == bridge::contract::AuthState::Failed)
                {
                    self.account = None;
                }
                self.cancel_needed = !self
                    .account
                    .as_ref()
                    .is_some_and(|account| account.state == bridge::contract::AuthState::SignedIn);
                self.dismissed = false;
                self.pending = Some(AccountOperation::SignIn);
            }
            AccountIntent::SignOut => {
                self.dismissed = true;
                self.cancel_needed = false;
                self.pending = Some(AccountOperation::SignOut);
            }
            AccountIntent::Cancel => {
                self.dismissed = true;
                // Cancel even before a device code arrives. Signing out invalidates the
                // relay's pending device flow, so a late result cannot authenticate it.
                if self.cancel_needed {
                    self.pending = Some(AccountOperation::SignOut);
                }
                self.cancel_needed = false;
            }
        }
    }

    /// Moves the newest intent into the existing Session's coalesced command slot.
    pub fn take_operation(&mut self) -> Option<AccountOperation> {
        self.pending.take()
    }

    /// Retires presentation belonging to an expired account before a fresh control session.
    pub fn fresh_session(&mut self) {
        *self = Self::default();
    }

    /// Restores presentation for an operation retained by the Session or its bounded host queue.
    pub fn restore_operation(&mut self, operation: AccountOperation) {
        self.request(match operation {
            AccountOperation::SignIn => AccountIntent::SignIn,
            AccountOperation::SignOut => AccountIntent::SignOut,
        });
        self.pending = None;
    }

    /// Applies typed relay observations; transport failures remain owned by the host.
    pub fn observe(&mut self, event: &ControlEvent) {
        match event {
            ControlEvent::Account(account) => {
                let progressed = self.account.as_ref() != Some(account)
                    && matches!(
                        account.state,
                        bridge::contract::AuthState::AwaitingCode
                            | bridge::contract::AuthState::SignedIn
                    );
                if matches!(self.failure, Some(Failure::Disconnected)) || progressed {
                    self.failure = None;
                }
                match account.state {
                    bridge::contract::AuthState::SignedIn => self.cancel_needed = false,
                    bridge::contract::AuthState::AwaitingCode if !self.dismissed => {
                        self.cancel_needed = true
                    }
                    _ => {}
                }
                self.account = Some(account.clone());
            }
            ControlEvent::RequestRefused(reason) => {
                self.failure = Some(Failure::Refused(reason.message()));
            }
            ControlEvent::Connect(progress) => {
                if progress.stage == bridge::contract::ConnectStage::Idle {
                    self.progress = None;
                    return;
                }
                self.progress = Some(match progress.stage {
                    bridge::contract::ConnectStage::Packs => JoinStage::Packs {
                        done: progress.packs_done,
                        total: progress.packs_total,
                        received_bytes: progress.received_bytes,
                        total_bytes: progress.total_bytes,
                    },
                    _ => JoinStage::Connecting,
                });
            }
            ControlEvent::Keepalive => {}
        }
    }

    /// Retires transient prompts and identity when the live relay becomes unavailable.
    pub fn disconnected(&mut self) {
        self.account = None;
        self.progress = None;
        self.failure = Some(Failure::Disconnected);
    }

    /// Projects the current account into the game's existing sign-in prompt states.
    pub fn state(&self) -> AuthState {
        if self
            .account
            .as_ref()
            .is_some_and(|account| account.state == bridge::contract::AuthState::SignedIn)
        {
            return AuthState::Authenticated;
        }
        if let Some(failure) = self.failure {
            return AuthState::Failed(failure.message().into());
        }
        let Some(account) = &self.account else {
            return if self.cancel_needed {
                AuthState::Checking
            } else {
                AuthState::SignedOut
            };
        };
        match account.state {
            bridge::contract::AuthState::SignedIn => AuthState::Authenticated,
            bridge::contract::AuthState::AwaitingCode if !self.dismissed => {
                match (&account.verification_uri, &account.user_code) {
                    (Some(uri), Some(code))
                        if verification_link(uri).is_some()
                            && !code.is_empty()
                            && code.len() <= 64
                            && ::launcher::menu::sign_in::device_code_url(code).is_some() =>
                    {
                        AuthState::AwaitingCode {
                            uri: uri.clone(),
                            code: code.clone(),
                        }
                    }
                    _ => AuthState::Failed("The relay returned an invalid sign-in prompt.".into()),
                }
            }
            bridge::contract::AuthState::Failed if !self.dismissed => {
                AuthState::Failed("Sign-in failed. Try again.".into())
            }
            _ if self.cancel_needed => AuthState::Checking,
            _ => AuthState::SignedOut,
        }
    }

    /// Reports refused operations separately from the confirmed authentication state.
    pub fn operation_error(&self) -> Option<&str> {
        self.failure.map(Failure::message)
    }

    /// Supplies only the identity the relay actually reports, leaving service details unavailable.
    pub fn profile(&self) -> MenuProfile {
        match self
            .account
            .as_ref()
            .filter(|account| account.state == bridge::contract::AuthState::SignedIn)
        {
            Some(account) => MenuProfile {
                gamertag: account.gamertag.clone().unwrap_or_default(),
                ..MenuProfile::unavailable()
            },
            None => MenuProfile::default(),
        }
    }

    /// Returns progress for the current join only.
    pub fn join_stage(&self) -> Option<JoinStage> {
        self.progress
    }

    /// Clears a previous attempt before the host opens a new play channel.
    pub fn begin_join(&mut self) {
        self.progress = None;
    }
}

/// Admits public addressed-server syntax; the relay remains the authority for egress policy.
pub fn addressed_target(address: &str) -> Result<ConnectTarget, &'static str> {
    let address = address.trim();
    if address.is_empty()
        || address.len() > MAX_SERVER_ADDRESS_BYTES
        || address.chars().any(char::is_whitespace)
        || address.contains(['/', '@', '?', '#', '\\'])
    {
        return Err(
            "Only addressed servers can be joined from a browser. Realms, friends' worlds and featured experiences are not available yet.",
        );
    }
    let (host, port) = ::launcher::menu::split_address(address);
    let port = port
        .parse::<u16>()
        .ok()
        .filter(|port| *port != 0)
        .ok_or("Enter a valid server port.")?;
    // URL parsing rejects malformed brackets and extra colons rather than appending a port.
    let host = if host.contains(':') {
        format!("[{host}]")
    } else {
        host
    };
    let url = url::Url::parse(&format!("https://{host}:{port}/"))
        .map_err(|_| "Enter a valid server address.")?;
    if url.host_str().is_none() || !url.username().is_empty() || url.password().is_some() {
        return Err("Enter a valid server address.");
    }
    Ok(ConnectTarget::RakNet(format!(
        "{}:{port}",
        url.host().ok_or("Enter a valid server address.")?
    )))
}

/// Validates relay links before either presentation or the synchronous gesture bridge uses them.
pub fn verification_link(uri: &str) -> Option<String> {
    if uri.len() > 2048 {
        return None;
    }
    let url = url::Url::parse(uri).ok()?;
    (url.scheme() == "https"
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none())
    .then(|| url.into())
}
