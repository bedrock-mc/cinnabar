//! The menu's account surface in a browser, whose web relay signs in and prepares joins over its
//! control socket ([`bridge::RelayControl`]) instead of a launcher core.

use launcher::menu::{
    auth::AuthState,
    view::{JoinStage, MenuFriendCard, MenuProfile, MenuRealmCard},
};

use crate::account_control::{AccountControl, AccountEvent};

/// Signs in through the relay and shows the joins it prepares.
pub struct RelayAccount {
    control: bridge::RelayControl,
    /// A sign-in this client asked for that the relay has not yet answered.
    signing_in: bool,
    /// Why the relay refused the last sign-in, shown until the relay's state moves on, with
    /// the state it refused in.
    refused: Option<(String, Option<bridge::Account>)>,
    /// The gamertag the menu's profile last showed.
    shown_gamertag: Option<String>,
}

impl RelayAccount {
    /// Reads and drives `control`'s account.
    #[must_use]
    pub fn new(control: bridge::RelayControl) -> Self {
        Self {
            control,
            signing_in: false,
            refused: None,
            shown_gamertag: None,
        }
    }
}

impl AccountControl for RelayAccount {
    fn account_status(&mut self) -> Option<AuthState> {
        let account = self.control.account();
        if let Some(reason) = self.control.take_refusal() {
            if std::mem::take(&mut self.signing_in) {
                self.refused = Some((refusal_message(&reason), account.clone()));
            } else {
                tracing::warn!(%reason, "the web relay refused a request");
            }
        }
        if let Some((message, refused_in)) = &self.refused {
            if *refused_in == account {
                return Some(AuthState::Failed(message.clone()));
            }
            self.refused = None;
        }
        let account = account?;
        // The relay answers a sign-in with a code, or with the state it already had.
        if self.signing_in {
            match account.state {
                bridge::AuthState::SignedOut | bridge::AuthState::Failed => {
                    return Some(AuthState::Checking);
                }
                _ => self.signing_in = false,
            }
        }
        crate::launcher_account::auth_state(&account)
    }

    fn realms(&mut self) -> Option<Vec<MenuRealmCard>> {
        None
    }

    fn friends(&mut self) -> Option<Vec<MenuFriendCard>> {
        None
    }

    fn sign_out(&mut self) -> bool {
        self.signing_in = false;
        self.control.sign_out();
        true
    }

    fn start_sign_in(&mut self) {
        self.refused = None;
        self.signing_in = true;
        self.control.sign_in();
    }

    fn cancel_sign_in(&mut self) {
        self.signing_in = false;
        // Signing out cancels a sign-in waiting for its code; a finished one stays.
        if self
            .control
            .account()
            .is_some_and(|account| account.state == bridge::AuthState::AwaitingCode)
        {
            self.control.sign_out();
        }
    }

    fn poll_event(&mut self) -> Option<AccountEvent> {
        None
    }

    fn profile(&mut self) -> Option<MenuProfile> {
        let gamertag = self
            .control
            .account()
            .filter(|account| account.state == bridge::AuthState::SignedIn)
            .and_then(|account| account.gamertag);
        if gamertag == self.shown_gamertag {
            return None;
        }
        self.shown_gamertag = gamertag.clone();
        // The relay names the account and nothing more, so the profile's details stay empty.
        Some(match gamertag {
            Some(gamertag) => MenuProfile {
                gamertag,
                ..MenuProfile::unavailable()
            },
            None => MenuProfile::default(),
        })
    }

    fn join_stage(&mut self) -> Option<JoinStage> {
        self.control
            .connect_progress()
            .map(|progress| crate::launcher_account::join_stage(&progress))
    }
}

/// What a player can do about the relay refusing `reason`.
fn refusal_message(reason: &str) -> String {
    match reason {
        "rate_limited" => "The relay is busy. Wait a moment, then try again.".to_owned(),
        "session_busy" => "Leave the server before signing in or out.".to_owned(),
        "too_many_sessions" => {
            "Too many Cinnabar tabs are open on this network. Close one, then try again.".to_owned()
        }
        other => format!("The relay refused that ({other})."),
    }
}
