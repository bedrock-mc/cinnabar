//! The update flow's states and the only transitions between them, free of I/O and clocks.

use serde::Serialize;
use update_manifest::channel::Channel;

/// What the launcher shows about updates; each install step is a separate state.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum UpdateState {
    Idle {
        #[serde(skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "typescript", ts(optional))]
        last_checked: Option<String>,
    },
    Checking,
    UpToDate {
        last_checked: String,
    },
    Available {
        version: String,
        channel: Channel,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "typescript", ts(optional))]
        notes_url: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "typescript", ts(optional))]
        size_bytes: Option<u64>,
        /// A server refused this build as outdated.
        required: bool,
        /// The selected channel's release is older than the installed build.
        downgrade: bool,
    },
    Downloading {
        version: String,
        received_bytes: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "typescript", ts(optional))]
        total_bytes: Option<u64>,
    },
    Verifying {
        version: String,
    },
    Installing {
        version: String,
    },
    /// This build cannot update itself, such as an unsigned development build; not a failure.
    Unavailable {
        reason: String,
    },
    Failed {
        message: String,
        /// The step that failed, so a retry repeats it.
        stage: UpdateStage,
        retryable: bool,
        can_play_installed: bool,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum UpdateStage {
    Check,
    Download,
    Verify,
    Install,
}

/// Why a check produced no offer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CheckError {
    Failed(String),
    Unavailable(String),
}

/// One verified release the selected channel offers; the stage is re-derived at install time.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Offer {
    pub version: String,
    pub build: u64,
    pub channel: Channel,
    pub notes_url: Option<String>,
    pub size: u64,
    pub downgrade: bool,
}

/// Owns the state plus a generation that retires work started before a channel change.
#[derive(Debug)]
pub(crate) struct Machine {
    state: UpdateState,
    generation: u64,
    offer: Option<Offer>,
    required: bool,
    last_checked: Option<String>,
}

impl Machine {
    pub fn new(initial: UpdateState) -> Self {
        Self {
            state: initial,
            generation: 0,
            offer: None,
            required: false,
            last_checked: None,
        }
    }

    pub fn state(&self) -> &UpdateState {
        &self.state
    }

    /// True while a download, verification or installation owns the stage.
    pub fn busy(&self) -> bool {
        matches!(
            self.state,
            UpdateState::Downloading { .. }
                | UpdateState::Verifying { .. }
                | UpdateState::Installing { .. }
        )
    }

    /// Starts a check unless one is running or an install owns the stage.
    pub fn begin_check(&mut self) -> Option<u64> {
        if self.busy() || self.state == UpdateState::Checking {
            return None;
        }
        self.generation += 1;
        self.state = UpdateState::Checking;
        Some(self.generation)
    }

    /// Publishes a check's result unless a channel change retired it.
    pub fn finish_check(
        &mut self,
        generation: u64,
        outcome: Result<Option<Offer>, CheckError>,
        now: &str,
    ) -> bool {
        if generation != self.generation {
            return false;
        }
        match outcome {
            Ok(offer) => {
                self.last_checked = Some(now.to_owned());
                self.offer = offer;
                self.state = self.offer_state();
            }
            Err(CheckError::Failed(message)) => {
                self.state = failed(message, UpdateStage::Check, true);
            }
            Err(CheckError::Unavailable(reason)) => {
                self.offer = None;
                self.state = UpdateState::Unavailable { reason };
            }
        }
        true
    }

    /// Claims the offered release for an explicit install, also after a retryable failure.
    pub fn begin_install(&mut self) -> Option<(u64, Offer)> {
        let ready = match &self.state {
            UpdateState::Available { .. } => true,
            UpdateState::Failed { retryable, .. } => *retryable,
            _ => false,
        };
        let offer = self.offer.clone().filter(|_| ready)?;
        self.generation += 1;
        self.state = UpdateState::Downloading {
            version: offer.version.clone(),
            received_bytes: 0,
            total_bytes: Some(offer.size),
        };
        Some((self.generation, offer))
    }

    pub fn progress(&mut self, generation: u64, received: u64, total: Option<u64>) -> bool {
        self.step(generation, |version| UpdateState::Downloading {
            version,
            received_bytes: received,
            total_bytes: total,
        })
    }

    pub fn verifying(&mut self, generation: u64) -> bool {
        self.step(generation, |version| UpdateState::Verifying { version })
    }

    /// Mutation is about to begin; from here only the installer's own recovery applies.
    pub fn installing(&mut self, generation: u64) -> bool {
        self.step(generation, |version| UpdateState::Installing { version })
    }

    /// Ends a check or install with an error; the installed build always stays playable.
    pub fn fail(&mut self, generation: u64, message: String, retryable: bool) -> bool {
        if generation != self.generation {
            return false;
        }
        let stage = match self.state {
            UpdateState::Verifying { .. } => UpdateStage::Verify,
            UpdateState::Installing { .. } => UpdateStage::Install,
            UpdateState::Checking => UpdateStage::Check,
            _ => UpdateStage::Download,
        };
        self.state = failed(message, stage, retryable);
        true
    }

    /// Returns to the offer after a handoff that needs no exit, such as an opened package.
    pub fn handed_off(&mut self, generation: u64) -> bool {
        if generation != self.generation {
            return false;
        }
        self.state = self.offer_state();
        true
    }

    /// Retires running work for a new channel; refused once installation has started.
    pub fn change_channel(&mut self) -> bool {
        if matches!(self.state, UpdateState::Installing { .. }) {
            return false;
        }
        self.generation += 1;
        self.offer = None;
        self.state = UpdateState::Idle { last_checked: None };
        self.last_checked = None;
        true
    }

    /// A server refused this build: the offer reads as required from now on.
    pub fn require(&mut self) {
        self.required = true;
        if let UpdateState::Available { required, .. } = &mut self.state {
            *required = true;
        }
    }

    #[cfg(debug_assertions)]
    pub fn show(&mut self, state: UpdateState) {
        self.state = state;
    }

    pub fn active(&self, generation: u64) -> bool {
        self.generation == generation
    }

    fn step(&mut self, generation: u64, next: impl FnOnce(String) -> UpdateState) -> bool {
        if generation != self.generation || !self.busy() {
            return false;
        }
        let Some(offer) = &self.offer else {
            return false;
        };
        self.state = next(offer.version.clone());
        true
    }

    fn offer_state(&self) -> UpdateState {
        match (&self.offer, &self.last_checked) {
            (Some(offer), _) => UpdateState::Available {
                version: offer.version.clone(),
                channel: offer.channel,
                notes_url: offer.notes_url.clone(),
                size_bytes: Some(offer.size),
                required: self.required && !offer.downgrade,
                downgrade: offer.downgrade,
            },
            (None, Some(checked)) => UpdateState::UpToDate {
                last_checked: checked.clone(),
            },
            (None, None) => UpdateState::Idle { last_checked: None },
        }
    }
}

fn failed(message: String, stage: UpdateStage, retryable: bool) -> UpdateState {
    UpdateState::Failed {
        message,
        stage,
        retryable,
        can_play_installed: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offer(version: &str) -> Offer {
        Offer {
            version: version.into(),
            build: 2,
            channel: Channel::Beta,
            notes_url: None,
            size: 100,
            downgrade: false,
        }
    }

    #[test]
    fn a_check_moves_from_idle_to_an_offer_or_up_to_date() {
        let mut machine = Machine::new(UpdateState::Idle { last_checked: None });
        let generation = machine.begin_check().unwrap();
        assert_eq!(machine.state(), &UpdateState::Checking);
        assert!(machine.begin_check().is_none(), "one check at a time");
        assert!(machine.finish_check(generation, Ok(None), "t1"));
        assert_eq!(
            machine.state(),
            &UpdateState::UpToDate {
                last_checked: "t1".into()
            }
        );
        let generation = machine.begin_check().unwrap();
        machine.finish_check(generation, Ok(Some(offer("2.0.0"))), "t2");
        assert!(matches!(
            machine.state(),
            UpdateState::Available { version, required: false, .. } if version == "2.0.0"
        ));
    }

    #[test]
    fn installing_needs_an_offer_and_walks_each_step_in_order() {
        let mut machine = Machine::new(UpdateState::Idle { last_checked: None });
        assert!(machine.begin_install().is_none(), "nothing offered yet");
        let generation = machine.begin_check().unwrap();
        machine.finish_check(generation, Ok(Some(offer("2.0.0"))), "t");
        let (install, claimed) = machine.begin_install().unwrap();
        assert_eq!(claimed.version, "2.0.0");
        assert!(
            machine.begin_check().is_none(),
            "a running install blocks checks"
        );
        assert!(machine.begin_install().is_none(), "one install at a time");
        assert!(machine.progress(install, 50, Some(100)));
        assert!(matches!(
            machine.state(),
            UpdateState::Downloading {
                received_bytes: 50,
                ..
            }
        ));
        assert!(machine.verifying(install));
        assert!(machine.installing(install));
        assert!(!machine.change_channel(), "mutation has started");
        assert!(matches!(machine.state(), UpdateState::Installing { .. }));
    }

    #[test]
    fn a_failed_install_keeps_the_game_playable_and_can_be_retried() {
        let mut machine = Machine::new(UpdateState::Idle { last_checked: None });
        let generation = machine.begin_check().unwrap();
        machine.finish_check(generation, Ok(Some(offer("2.0.0"))), "t");
        let (install, _) = machine.begin_install().unwrap();
        machine.verifying(install);
        machine.fail(install, "hash mismatch".into(), true);
        assert!(matches!(
            machine.state(),
            UpdateState::Failed {
                stage: UpdateStage::Verify,
                can_play_installed: true,
                retryable: true,
                ..
            }
        ));
        assert!(machine.begin_install().is_some());
    }

    #[test]
    fn a_channel_change_retires_running_checks_and_downloads() {
        let mut machine = Machine::new(UpdateState::Idle { last_checked: None });
        let stale = machine.begin_check().unwrap();
        assert!(machine.change_channel());
        assert!(!machine.finish_check(stale, Ok(Some(offer("9.0.0"))), "t"));
        assert_eq!(machine.state(), &UpdateState::Idle { last_checked: None });
        let generation = machine.begin_check().unwrap();
        machine.finish_check(generation, Ok(Some(offer("2.0.0"))), "t");
        let (install, _) = machine.begin_install().unwrap();
        assert!(machine.change_channel());
        assert!(!machine.progress(install, 1, None));
        assert!(!machine.installing(install));
    }

    #[test]
    fn an_outdated_client_refusal_marks_only_upgrades_required() {
        let mut machine = Machine::new(UpdateState::Idle { last_checked: None });
        machine.require();
        let generation = machine.begin_check().unwrap();
        machine.finish_check(generation, Ok(Some(offer("2.0.0"))), "t");
        assert!(matches!(
            machine.state(),
            UpdateState::Available { required: true, .. }
        ));
        let generation = machine.begin_check().unwrap();
        machine.finish_check(
            generation,
            Ok(Some(Offer {
                downgrade: true,
                ..offer("0.9.0")
            })),
            "t",
        );
        assert!(matches!(
            machine.state(),
            UpdateState::Available {
                required: false,
                downgrade: true,
                ..
            }
        ));
    }

    #[test]
    fn a_build_that_cannot_update_is_unavailable_rather_than_failed() {
        let mut machine = Machine::new(UpdateState::Idle { last_checked: None });
        let generation = machine.begin_check().unwrap();
        machine.finish_check(
            generation,
            Err(CheckError::Unavailable("unsigned".into())),
            "t",
        );
        assert_eq!(
            machine.state(),
            &UpdateState::Unavailable {
                reason: "unsigned".into()
            }
        );
        assert!(machine.begin_install().is_none());
    }

    #[test]
    fn states_serialize_with_the_launcher_contract_names() {
        let state = UpdateState::Available {
            version: "2.0.0".into(),
            channel: Channel::Nightly,
            notes_url: None,
            size_bytes: Some(5),
            required: false,
            downgrade: false,
        };
        assert_eq!(
            serde_json::to_value(&state).unwrap(),
            serde_json::json!({
                "kind": "available",
                "version": "2.0.0",
                "channel": "nightly",
                "sizeBytes": 5,
                "required": false,
                "downgrade": false,
            })
        );
        assert_eq!(
            serde_json::to_value(UpdateState::UpToDate {
                last_checked: "t".into()
            })
            .unwrap(),
            serde_json::json!({ "kind": "upToDate", "lastChecked": "t" })
        );
    }
}
