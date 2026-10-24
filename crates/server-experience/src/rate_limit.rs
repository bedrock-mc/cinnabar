//! Clock-driven optional-channel admission, independent of host fetch and execution services.
use crate::policy::{MAX_BURST_BYTES, MAX_BYTES_PER_SECOND, MAX_MESSAGES_PER_SECOND};
use anyhow::{Result, ensure};

// BEGIN GENERATED EXTENSION CONTRACT
// Generated from crates/server-experience/schema/extension.json; do not edit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RateClass {
    Data,
    NativeEpoch,
    WorldNotice,
    WorldAck,
    Handshake,
}
impl RateClass {
    /// Counts encoded carrier bytes and messages, including JSON escaping and fragment headers.
    pub fn cost(self, sizes: impl IntoIterator<Item = usize>) -> anyhow::Result<RateCost> {
        let mut cost = RateCost::default();
        for size in sizes {
            anyhow::ensure!(
                size <= protocol::MAX_EXPERIENCE_ENVELOPE_BYTES,
                BurstExceeded
            );
            cost.messages += 1;
            cost.bytes += size as u64;
            if cost.messages > crate::policy::MAX_MESSAGES_PER_SECOND
                || cost.bytes > crate::policy::MAX_BURST_BYTES
            {
                return Err(BurstExceeded.into());
            }
        }
        match self {
            Self::Data => Ok(cost),
            Self::NativeEpoch => Ok(cost),
            Self::WorldNotice => Ok(RateCost::default()),
            Self::WorldAck => Ok(RateCost::default()),
            Self::Handshake => Ok(RateCost::default()),
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RateCost {
    pub messages: u64,
    pub bytes: u64,
}
/// A batch that cannot fit even when the rate bucket is full.
#[derive(Debug)]
pub struct BurstExceeded;
impl std::fmt::Display for BurstExceeded {
    /// Explains why retrying the unchanged batch cannot succeed.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("encoded message too large for maximum rate burst")
    }
}
impl std::error::Error for BurstExceeded {}
#[cfg(feature = "host")]
impl crate::session::Control {
    /// Classifies controls by direction; acknowledgments are exempt only for an outstanding notice.
    pub fn rate_class(
        &self,
        direction: crate::wire::Direction,
        matching_notice: bool,
    ) -> RateClass {
        use crate::wire::Direction;
        match self {
            Self::Hello(..) => RateClass::Handshake,
            Self::Accept(..) => RateClass::Handshake,
            Self::Ready { .. } => RateClass::Handshake,
            Self::Epoch { .. } => match direction {
                Direction::ToClient => RateClass::WorldNotice,
                Direction::ToServer => RateClass::NativeEpoch,
            },
            Self::WorldAck { .. } => {
                if direction == Direction::ToServer && matching_notice {
                    return RateClass::WorldAck;
                }
                RateClass::Data
            }
            Self::Disabled => RateClass::Handshake,
        }
    }
}
#[cfg(feature = "host")]
impl crate::negotiation::Limits {
    /// Lowers the payload ceiling so UTF-8 fragmentation fits the message burst.
    pub fn fit_rate(mut self) -> Self {
        self.max_message_bytes = self.max_message_bytes.min(
            self.max_fragment_bytes
                .saturating_sub(3)
                .saturating_mul(crate::policy::MAX_MESSAGES_PER_SECOND as u32),
        );
        self
    }
}
// END GENERATED EXTENSION CONTRACT

/// Temporary credit exhaustion; retrying after a refill may succeed.
#[derive(Debug)]
pub struct RateExceeded;

impl std::fmt::Display for RateExceeded {
    /// Describes a temporary admission refusal.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("channel rate exceeded")
    }
}

impl std::error::Error for RateExceeded {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RateLimit {
    last_ms: u64,
    messages: u64,
    bytes: u64,
}

impl RateLimit {
    /// Gives the shared initial burst credit.
    pub fn new(now_ms: u64) -> Self {
        Self {
            last_ms: now_ms,
            messages: MAX_MESSAGES_PER_SECOND * 1000,
            bytes: MAX_BURST_BYTES * 1000,
        }
    }

    /// Charges before parsing; a clock reversal cannot mint extra credit.
    pub fn charge(&mut self, size: usize, now_ms: u64) -> Result<()> {
        self.charge_class(RateClass::Data, size, now_ms)
    }

    /// Applies the shared classification and encoded size before spending any credit.
    pub fn charge_class(&mut self, class: RateClass, size: usize, now_ms: u64) -> Result<()> {
        self.spend(class.cost([size])?, now_ms)
    }

    /// Charges every encoded fragment together or leaves the batch waiting for credit.
    pub fn charge_all(
        &mut self,
        sizes: impl IntoIterator<Item = usize>,
        now_ms: u64,
    ) -> Result<()> {
        self.spend(RateClass::Data.cost(sizes)?, now_ms)
    }

    /// Refills the bucket and spends a cost already checked against the maximum burst.
    fn spend(&mut self, cost: RateCost, now_ms: u64) -> Result<()> {
        let elapsed = now_ms
            .saturating_sub(self.last_ms)
            .min((MAX_BURST_BYTES / MAX_BYTES_PER_SECOND + 1) * 1000);
        self.last_ms = self.last_ms.max(now_ms);
        self.messages =
            (self.messages + elapsed * MAX_MESSAGES_PER_SECOND).min(MAX_MESSAGES_PER_SECOND * 1000);
        self.bytes = (self.bytes + elapsed * MAX_BYTES_PER_SECOND).min(MAX_BURST_BYTES * 1000);
        ensure!(
            self.messages >= cost.messages * 1000 && self.bytes >= cost.bytes * 1000,
            RateExceeded
        );
        self.messages -= cost.messages * 1000;
        self.bytes -= cost.bytes * 1000;
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
        assert!(
            rate.charge_all(
                std::iter::repeat_n(
                    crate::policy::MAX_PAYLOAD_BYTES,
                    MAX_BURST_BYTES as usize / crate::policy::MAX_PAYLOAD_BYTES
                ),
                2000
            )
            .is_err()
        );
        assert!(
            rate.charge(protocol::MAX_EXPERIENCE_ENVELOPE_BYTES + 1, 3000)
                .is_err()
        );
    }
}

#[cfg(all(test, feature = "host"))]
#[path = "rate_limit/tests.rs"]
mod contract_tests;
