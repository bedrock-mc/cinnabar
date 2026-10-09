//! Preview and explicit acceptance of a Realm invitation, independent of joining its world.

use super::MenuRealmCard;

/// A press on the Realm membership flow.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    Open,
    EditCode,
    Verify,
    Accept,
    Back,
    Play,
}

/// The currently visible invitation state.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Stage {
    #[default]
    Code,
    Verifying,
    Confirm,
    Joining,
    Complete,
}

/// The invitation draft and the service's preview. Membership needs an explicit confirmation.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct State {
    pub code: String,
    pub stage: Stage,
    pub realm: Option<MenuRealmCard>,
    pub error: Option<&'static str>,
}

impl State {
    /// Only an idle, nonempty draft can be verified.
    pub fn can_verify(&self) -> bool {
        self.stage == Stage::Code && !self.code.trim().is_empty()
    }

    /// Admits one request at a time and retains the confirmed code for acceptance.
    pub fn begin(&mut self, accept: bool) -> bool {
        let ready = if accept {
            self.stage == Stage::Confirm && self.realm.is_some()
        } else {
            self.can_verify()
        };
        if !ready {
            return false;
        }
        self.error = None;
        self.stage = if accept {
            Stage::Joining
        } else {
            Stage::Verifying
        };
        true
    }

    /// Publishes the matching operation's answer; failure allows a retry without accepting anything.
    pub fn finish(&mut self, accept: bool, result: Result<(String, MenuRealmCard), ()>) -> bool {
        if self.stage
            != if accept {
                Stage::Joining
            } else {
                Stage::Verifying
            }
        {
            return false;
        }
        match result {
            Ok((code, realm)) => {
                self.code = code;
                self.realm = Some(realm);
                self.stage = if accept {
                    Stage::Complete
                } else {
                    Stage::Confirm
                };
                true
            }
            Err(()) => {
                self.stage = if accept { Stage::Confirm } else { Stage::Code };
                self.error = Some(if accept {
                    "Unable to join this Realm. Please try again."
                } else {
                    "Unable to verify this Realm invitation. Check the link or code and try again."
                });
                false
            }
        }
    }

    /// Acceptance is not cancellable once submitted; its outcome must be observed.
    pub fn can_back(&self) -> bool {
        self.stage != Stage::Joining
    }

    /// The controls reachable by keyboard while this flow covers the Realms tab.
    pub fn actions(&self) -> Vec<Action> {
        match self.stage {
            Stage::Code => [
                Some(Action::EditCode),
                self.can_verify().then_some(Action::Verify),
                Some(Action::Back),
            ]
            .into_iter()
            .flatten()
            .collect(),
            Stage::Verifying => vec![Action::Back],
            Stage::Confirm => vec![Action::Accept, Action::Back],
            Stage::Joining => Vec::new(),
            Stage::Complete => vec![Action::Play, Action::Back],
        }
    }
}
