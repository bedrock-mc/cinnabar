//! Connection-scoped negotiation and immediate revocation.

use crate::{
    crypto::SignedDocument,
    negotiation::{Grant, Hello, Pending, VerifiedOffer},
    policy::*,
    trust::{Choice, Decision, Settings},
    wire::RateLimit,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

#[derive(Debug, Deserialize, Serialize)]
#[serde(
    tag = "kind",
    content = "body",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Control {
    Hello(Hello),
    Accept(SignedDocument),
    Ready {
        session: String,
        packages: Vec<String>,
        generation: u64,
        permissions: std::collections::BTreeMap<
            String,
            std::collections::BTreeSet<crate::manifest::Permission>,
        >,
        world_epoch: u64,
    },
    /// Wire v2: the client's world epoch changed and its runtime kept running. Later
    /// envelopes carry `world_epoch`; earlier-epoch ones are dropped and counted, not fatal.
    Epoch {
        session: String,
        world_epoch: u64,
    },
    Disabled,
}

/// Session snapshots share one consumption latch instead of copying the challenge.
#[derive(Clone, Debug)]
pub struct SharedPending(Arc<Mutex<Option<Pending>>>);

impl SharedPending {
    /// Consumes the challenge exactly once across every session snapshot.
    fn accept(self, document: &SignedDocument, now_unix: u64, now_ms: u64) -> Result<Grant> {
        let pending = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("pending handshake poisoned"))?
            .take()
            .ok_or_else(|| anyhow::anyhow!("handshake already consumed"))?;
        pending.accept(document, now_unix, now_ms)
    }
}

#[derive(Clone, Debug, Default)]
pub enum State {
    #[default]
    Inert,
    Offered(VerifiedOffer),
    Awaiting(SharedPending),
    Granted(Grant),
    Disabled,
}

#[derive(Clone, Debug, Default)]
pub struct Session {
    pub state: State,
    pub notice: Option<String>,
    pub key_changed: bool,
    rate: Option<RateLimit>,
    started_ms: u64,
    outbound: Option<Vec<u8>>,
}

impl Session {
    /// Consumes one inert marker; declines cannot trigger repeated prompts.
    pub fn discover(
        &mut self,
        bytes: &[u8],
        audience: &str,
        settings: &mut Settings,
        now_unix: u64,
        now_ms: u64,
    ) -> Result<bool> {
        ensure!(matches!(self.state, State::Inert), "offer already handled");
        let offer = VerifiedOffer::read(bytes, audience, now_unix)?;
        ensure!(
            !settings
                .pins
                .iter()
                .any(|pin| pin.audience == offer.offer.audience
                    && pin.server_key == offer.offer.server_key
                    && pin.highest_revision > offer.offer.revision),
            "deployment rollback denied"
        );
        let decision = settings.decision(&offer.offer)?;
        self.key_changed = settings.key_changed(&offer.offer);
        let remember = decision == Some(Decision::Always);
        if remember {
            settings.remember(&offer.offer, Decision::Always)?;
        }
        self.state = match decision {
            Some(Decision::Never) => State::Disabled,
            _ => State::Offered(offer),
        };
        if remember {
            self.approve(now_ms)?;
        }
        Ok(remember)
    }

    /// Applies only host UI actions; no remote operation can create consent.
    pub fn choose(&mut self, choice: Choice, settings: &mut Settings, now_ms: u64) -> Result<bool> {
        if choice == Choice::Disable {
            self.disable();
            return Ok(false);
        }
        let State::Offered(offer) = &self.state else {
            return Ok(false);
        };
        let persist = match choice {
            Choice::Always => {
                settings.remember(&offer.offer, Decision::Always)?;
                true
            }
            Choice::Never => {
                settings.remember(&offer.offer, Decision::Never)?;
                true
            }
            _ => false,
        };
        match choice {
            Choice::Once | Choice::Always => self.approve(now_ms)?,
            _ => self.disable(),
        }
        Ok(persist)
    }

    /// Generates traffic only after explicit or previously pinned consent.
    fn approve(&mut self, now_ms: u64) -> Result<()> {
        let State::Offered(offer) = std::mem::replace(&mut self.state, State::Disabled) else {
            return Ok(());
        };
        let pending = Pending::approve(offer, 0, now_ms)?;
        self.outbound = Some(serde_json::to_vec(&Control::Hello(
            pending.hello().clone(),
        ))?);
        self.state = State::Awaiting(SharedPending(Arc::new(Mutex::new(Some(pending)))));
        self.rate = Some(RateLimit::new(now_ms));
        self.started_ms = now_ms;
        Ok(())
    }

    /// Ignores unsolicited traffic and closes only this extension on bad control data.
    pub fn receive(&mut self, bytes: &[u8], now_unix: u64, now_ms: u64) -> Result<()> {
        if !matches!(self.state, State::Awaiting(_)) {
            return Ok(());
        }
        let result = self.accept(bytes, now_unix, now_ms);
        if result.is_err() {
            self.disable();
        }
        result
    }

    /// Charges before parsing and consumes the outstanding challenge once.
    fn accept(&mut self, bytes: &[u8], now_unix: u64, now_ms: u64) -> Result<()> {
        self.rate
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("no consent"))?
            .charge(bytes.len(), now_ms)?;
        let Control::Accept(document) = serde_json::from_slice(bytes)? else {
            anyhow::bail!("unexpected negotiation message");
        };
        let State::Awaiting(pending) = std::mem::replace(&mut self.state, State::Disabled) else {
            anyhow::bail!("no pending handshake");
        };
        self.state = State::Granted(pending.accept(&document, now_unix, now_ms)?);
        Ok(())
    }

    /// Expires permission without changing the ordinary game connection.
    pub fn tick(&mut self, now_unix: u64, now_ms: u64) {
        let expired = match &self.state {
            State::Awaiting(_) => now_ms.saturating_sub(self.started_ms) > NEGOTIATION_TIMEOUT_MS,
            State::Granted(grant) => now_unix >= grant.expires_unix,
            State::Offered(offer) => now_unix >= offer.offer.expires_unix,
            _ => false,
        };
        if expired {
            self.disable();
        }
    }

    /// Revokes pending output as well as live grants; no disable packet is required.
    pub fn disable(&mut self) {
        if let State::Awaiting(pending) = &self.state
            && let Ok(mut shared) = pending.0.lock()
        {
            shared.take();
        }
        self.state = State::Disabled;
        self.outbound = None;
        self.rate = None;
    }

    /// Takes only an already authorized handshake packet.
    pub fn take_outbound(&mut self) -> Option<Vec<u8>> {
        self.outbound.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_marker_is_byte_silent_under_unsolicited_traffic_and_input() {
        let mut session = Session::default();
        let mut settings = Settings::default();
        for bytes in [b"garbage".as_slice(), br#"{"kind":"disabled"}"#] {
            session.receive(bytes, 1000, 1000).unwrap();
            session.choose(Choice::Always, &mut settings, 1000).unwrap();
            session.tick(1000, 1000);
            assert!(session.take_outbound().is_none());
            assert!(matches!(session.state, State::Inert));
            assert!(settings.pins.is_empty());
        }
    }
}
