//! Clock-driven optional-channel admission, independent of host fetch and execution services.
use crate::policy::{MAX_BYTES_PER_SECOND, MAX_MESSAGES_PER_SECOND};
use anyhow::{Result, ensure};

#[derive(Clone, Debug)]
pub struct RateLimit {
    last_ms: u64,
    messages: u64,
    bytes: u64,
}

impl RateLimit {
    /// Gives at most one second of initial burst credit.
    pub fn new(now_ms: u64) -> Self {
        Self {
            last_ms: now_ms,
            messages: MAX_MESSAGES_PER_SECOND * 1000,
            bytes: MAX_BYTES_PER_SECOND * 1000,
        }
    }

    /// Charges before parsing; a clock reversal cannot mint extra credit.
    pub fn charge(&mut self, size: usize, now_ms: u64) -> Result<()> {
        let elapsed = now_ms.saturating_sub(self.last_ms).min(1000);
        self.last_ms = self.last_ms.max(now_ms);
        self.messages =
            (self.messages + elapsed * MAX_MESSAGES_PER_SECOND).min(MAX_MESSAGES_PER_SECOND * 1000);
        self.bytes = (self.bytes + elapsed * MAX_BYTES_PER_SECOND).min(MAX_BYTES_PER_SECOND * 1000);
        ensure!(
            size <= protocol::MAX_EXPERIENCE_ENVELOPE_BYTES,
            "envelope too large"
        );
        let cost = size as u64 * 1000;
        ensure!(
            self.messages >= 1000 && self.bytes >= cost,
            "channel rate exceeded"
        );
        self.messages -= 1000;
        self.bytes -= cost;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reversal_cannot_create_credit_and_refill_stays_bounded() {
        let mut rate = RateLimit::new(1000);
        for _ in 0..MAX_MESSAGES_PER_SECOND {
            rate.charge(1, 1000).unwrap();
        }
        assert!(rate.charge(1, 999).is_err());
        rate.charge(1, 2000).unwrap();
        assert!(rate.charge(MAX_BYTES_PER_SECOND as usize, 2000).is_err());
        assert!(
            rate.charge(protocol::MAX_EXPERIENCE_ENVELOPE_BYTES + 1, 3000)
                .is_err()
        );
    }
}
