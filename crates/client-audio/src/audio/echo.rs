//! Pairs a sound the client voices itself with the server's copy of the same action.

/// Which side voiced a sound both may produce.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EchoOrigin {
    /// Local prediction or an actor status event the client voices itself.
    Client,
    /// An explicit server sound packet.
    Packet,
}

/// What one voiced action is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EchoSubject {
    Cell([i32; 3]),
    Actor(i64),
}

struct Echo {
    event: Box<str>,
    subject: EchoSubject,
    origin: EchoOrigin,
    at: f64,
}

const MAX_ECHOES: usize = 64;

#[derive(Default)]
pub struct EchoLedger {
    echoes: Vec<Echo>,
}

impl EchoLedger {
    /// Whether a sound should play: false, consuming the match, when the other origin voiced the
    /// same event for the same subject within `window` seconds.
    pub fn admit(
        &mut self,
        origin: EchoOrigin,
        event: &str,
        subject: EchoSubject,
        window: f64,
        now: f64,
    ) -> bool {
        self.echoes.retain(|echo| now - echo.at <= window.max(1.0));
        if let Some(index) = self.echoes.iter().position(|echo| {
            echo.origin != origin
                && echo.subject == subject
                && &*echo.event == event
                && now - echo.at <= window
        }) {
            self.echoes.remove(index);
            return false;
        }
        if self.echoes.len() >= MAX_ECHOES {
            self.echoes.remove(0);
        }
        self.echoes.push(Echo {
            event: event.into(),
            subject,
            origin,
            at: now,
        });
        true
    }

    pub fn clear(&mut self) {
        self.echoes.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A 3-block class-wide window swallowed nearby placements, breaks and other actors' hurts.
    #[test]
    fn only_the_other_origins_copy_of_one_action_is_suppressed() {
        let mut ledger = EchoLedger::default();
        let (client, packet) = (EchoOrigin::Client, EchoOrigin::Packet);
        let cell = |x| EchoSubject::Cell([x, 64, 0]);
        assert!(ledger.admit(packet, "break", cell(0), 0.6, 0.0));
        assert!(
            ledger.admit(packet, "break", cell(1), 0.6, 0.1),
            "adjacent cell"
        );
        assert!(
            ledger.admit(packet, "break", cell(0), 0.6, 0.2),
            "server repeat"
        );
        assert!(ledger.admit(client, "place", cell(5), 0.6, 0.3));
        assert!(!ledger.admit(packet, "place", cell(5), 0.6, 0.4), "echo");
        assert!(
            ledger.admit(packet, "place", cell(5), 0.6, 0.5),
            "echo consumed"
        );
        assert!(ledger.admit(client, "hurt", EchoSubject::Actor(1), 0.4, 0.5));
        assert!(ledger.admit(packet, "hurt", EchoSubject::Actor(2), 0.4, 0.6));
        assert!(ledger.admit(client, "hurt", EchoSubject::Actor(3), 0.4, 1.0));
        assert!(
            ledger.admit(packet, "hurt", EchoSubject::Actor(3), 0.4, 1.5),
            "expired"
        );
    }
}
