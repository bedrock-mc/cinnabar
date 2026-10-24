//! BDS caller ordering after real Login, Initialized and the original live marker.

use super::*;
use crate::native_account::hosted_world::serve::play::test_support::BdsPrefixObservation;

#[derive(Debug)]
pub(super) enum Outcome {
    Played(Result<(), io::ErrorKind>, Result<(), io::ErrorKind>),
    Prefix(BdsPrefixObservation),
}

/// Uses current-thread execution so the first WouldBlock-to-port observation does not yield.
#[tokio::test]
async fn bds_closing_forwards_the_original_accepted_prefix_before_physical_finish() {
    run(true).await;
}

/// Checks all phase/lifetime facts only after the original complete fixture cleanup.
pub(super) fn assert_observed(observed: BdsPrefixObservation) {
    assert!(
        observed.received.iter().all(|packet| !packet.is_empty())
            && observed.received[0] != observed.received[1],
        "the actual received pair was not retained: {observed:?}"
    );
    assert!(
        observed.forwarding,
        "original output obligation was lost: {observed:?}"
    );
    assert_eq!(
        observed.first_finish,
        Some(io::ErrorKind::WouldBlock),
        "{observed:?}"
    );
    assert!(
        observed.socket_held_before_forwarding,
        "BDS finished its transport before forwarding the original prefix: {observed:?}"
    );
    assert_eq!(observed.closed, None, "{observed:?}");
    assert!(
        observed.finished && observed.socket_released_at_completion,
        "{observed:?}"
    );
}
