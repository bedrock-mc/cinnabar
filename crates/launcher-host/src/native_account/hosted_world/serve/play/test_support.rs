//! Observations on the original closing peer before its local transport retirement.

use super::{ClosingHostedPeer, Output};
use std::io;

mod bds_prefix;
pub(in crate::native_account::hosted_world) use bds_prefix::{
    BdsPrefixObservation, close_bds_prefix,
};

/// Records a retained forwarding obligation and the same wire's first error under its closing cutoff.
/// Missing established ownership is unverified; a receive fault is never followed to EOF.
pub(in crate::native_account::hosted_world) async fn first_incoming_terminal(
    closing: &mut ClosingHostedPeer,
) -> Option<(bool, io::Error)> {
    if closing.retired.is_some() {
        return None;
    }
    // Receive/Idle may already have consumed a fault; forwarding progress retains incoming faults.
    let forwarding = matches!(&closing.output, Output::Forwarding(_));
    let deadline = closing.deadline;
    let wire = closing.wire.as_mut()?;
    loop {
        if let Err(error) = wire.receive_raw(deadline).await {
            return Some((forwarding, error));
        }
    }
}
