//! Outbound server latency-probe echo transform.
//!
//! One shared scaling helper backs every play-phase probe answer site so the
//! plain and blob-cache ingress paths cannot drift apart.

/// Scales one from-server latency-probe creation time into the outbound echo
/// value using saturating arithmetic, so an extreme probe stays finite instead
/// of wrapping.
///
/// PROVISIONAL transform pending authoritative retail-client measurement:
///
/// - The previous exact echo matched gophertunnel-family relays and local BDS,
///   which accept any echoed creation time (BDS reads this packet only for its
///   ping display and has shown no validation of the value).
/// - One third-party anti-cheat implementation divides each echoed id by 1,000
///   twice for non-PlayStation devices before matching, so exact echoes never
///   resolve there; its PAI-ticked watchdog then tears the session down after
///   spawn streaming completes.
/// - Multiplying the received timestamp by 10^6 (`1_000_000`) is the documented
///   inverse of that normalization. What an authoritative vanilla client
///   actually sends remains unmeasured; an earlier retail-binary inspection
///   was inconclusive.
/// - Local BDS acceptance of this scaled value must be re-verified live before
///   any acceptance gate may cite it.
#[must_use]
pub const fn scaled_creation_time(creation_time: u64) -> u64 {
    creation_time.saturating_mul(1_000_000)
}

#[cfg(test)]
mod tests {
    use super::scaled_creation_time;

    #[test]
    fn extreme_probes_saturate_and_stay_finite() {
        assert_eq!(scaled_creation_time(u64::MAX), u64::MAX);

        // The largest input whose product still fits exactly...
        assert_eq!(
            scaled_creation_time(u64::MAX / 1_000_000),
            u64::MAX / 1_000_000 * 1_000_000,
            "the largest exact product must not saturate"
        );
        // ...and the first input past that boundary saturates instead of
        // wrapping to a small value.
        assert_eq!(scaled_creation_time(u64::MAX / 1_000_000 + 1), u64::MAX);

        // A representative ordinary timestamp scales exactly and stays finite.
        assert_eq!(scaled_creation_time(777), 777_000_000);
    }
}
