use crate::ProtocolError;

/// Wakes the play pump after a decoded server boundary has been retained.
///
/// The pump consumes the retained transfer or disconnect before it considers
/// this vendor-neutral sentinel an ordinary receive failure.
pub(super) fn boundary_wakeup<T>() -> Result<T, ProtocolError> {
    Err(ProtocolError::SessionBoundary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoded_session_boundary_wakes_the_world_receiver_immediately() {
        assert!(matches!(
            boundary_wakeup::<crate::WorldEvent>(),
            Err(ProtocolError::SessionBoundary)
        ));
    }
}
