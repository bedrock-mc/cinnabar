//! Observes only original received fixture packets, without accepting replacement input.

use super::*;

/// Reads two actual packets and restores that exact pair ahead of unread siblings on the same FIFO.
/// Requires no outstanding source cursor; this is retained accepted input, not unread wire data.
pub async fn retain_hosted_received_pair(
    upstream: &mut NativeHostedUpstream,
) -> io::Result<[Bytes; 2]> {
    if upstream.connection.current_raw().is_some() {
        return Err(invalid());
    }
    let first = upstream.receive().await?;
    let second = match upstream.receive().await {
        Ok(second) => second,
        Err(error) => {
            upstream.connection.prepend_raw(vec![first]);
            return Err(error);
        }
    };
    upstream.connection.finish_current();
    upstream
        .connection
        .prepend_raw(vec![first.clone(), second.clone()]);
    Ok([first, second])
}
