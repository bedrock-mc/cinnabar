//! Native server latency-probe timestamp conversion.

/// Converts the wire milliseconds into vanilla's fixed-width nanosecond value.
/// The native packet reader multiplies by one million; its writer emits that
/// value unchanged.
#[must_use]
pub const fn scaled_creation_time(creation_time: u64) -> u64 {
    creation_time.wrapping_mul(1_000_000)
}

/// Builds the echo after preceding world controls have been applied.
#[must_use]
pub fn network_stack_latency_reply(creation_time: u64) -> crate::Packet {
    valentine::bedrock::version::v1_26_51::NetworkStackLatencyPacket {
        creation_time: scaled_creation_time(creation_time),
        is_from_server: true,
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::scaled_creation_time;

    #[test]
    fn creation_time_matches_native_fixed_width_scaling() {
        assert_eq!(scaled_creation_time(777), 777_000_000);
        assert_eq!(scaled_creation_time(u64::MAX), u64::MAX - 999_999);
        assert_eq!(
            scaled_creation_time(u64::MAX / 1_000_000),
            u64::MAX / 1_000_000 * 1_000_000,
        );
        assert_eq!(scaled_creation_time(u64::MAX / 1_000_000 + 1), 448_384);
    }
}
