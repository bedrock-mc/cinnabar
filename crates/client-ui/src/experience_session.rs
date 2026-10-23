//! Trusted session controller; remote data never supplies consent controls.

use server_experience::session::Session;
use std::sync::Arc;

#[derive(Clone, Debug, Default)]
pub struct ExperienceSession {
    pub audience: Option<String>,
    pub marker: Option<Arc<[u8]>>,
    pub session: Session,
    pub handled_marker: bool,
    pub active: bool,
    pub epoch: u64,
    pub incoming: std::collections::VecDeque<(u64, protocol::ExperienceMessage)>,
    incoming_bytes: usize,
}

impl ExperienceSession {
    /// Ends every permission while retaining the host-selected destination.
    pub fn reset(&mut self) {
        self.marker = None;
        self.session = Session::default();
        self.handled_marker = false;
        self.active = false;
        self.incoming.clear();
        self.incoming_bytes = 0;
    }

    /// Binds a host-selected address, never an address inside a pack or packet.
    pub fn select_destination(&mut self, address: &str) {
        self.audience = server_experience::negotiation::canonical_audience(address).ok();
    }

    /// Consumes only control messages that crossed the world publication barrier.
    pub fn receive(&mut self, message: protocol::ExperienceMessage, now_ms: u64) {
        if self.active {
            if self.incoming.len() >= server_experience::policy::MAX_QUEUE_MESSAGES
                || message.as_bytes().len()
                    > server_experience::policy::MAX_QUEUE_BYTES - self.incoming_bytes
            {
                self.session.disable();
                self.active = false;
                self.incoming.clear();
                self.incoming_bytes = 0;
            } else {
                self.incoming_bytes += message.as_bytes().len();
                self.incoming.push_back((now_ms, message));
            }
            return;
        }
        if let Err(error) = self
            .session
            .receive(message.as_bytes(), unix_seconds(), now_ms)
        {
            tracing::warn!(%error, "server experience control rejected");
        }
    }

    /// Releases bounded ingress accounting when the runtime consumes a message.
    pub fn pop(&mut self) -> Option<(u64, protocol::ExperienceMessage)> {
        let entry = self.incoming.pop_front()?;
        self.incoming_bytes -= entry.1.as_bytes().len();
        Some(entry)
    }

    /// Holds the join on its loading screen while consent can still be asked; never once
    /// approved, since the server reads the hello only after the client enters the world.
    pub fn holds_world_entry(&self) -> bool {
        (self.marker.is_some() && !self.handled_marker)
            || matches!(
                self.session.state,
                server_experience::session::State::Offered(_)
            )
    }
}

/// Uses wall time only for signed expiration, never for media presentation.
pub fn unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(u64::MAX, |time| time.as_secs())
}
