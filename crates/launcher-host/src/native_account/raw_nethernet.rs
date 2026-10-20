//! One explicit raw service intent on the original selected account and protected socket owner.

use super::{prepare::check_preparation_deadline, read_identity::ReadIdentity, *};
use protocol::NativeRawTarget;

impl NativeAccount {
    /// Mints and opens the explicitly selected signaling dialect without joining a friend world.
    pub(super) async fn prepare_raw_nethernet(
        self: Arc<Self>,
        target: NativeRawTarget,
        claims: Value,
        random_id: protocol::ClientRandomId,
    ) -> io::Result<(
        protocol::NativeConnectionTarget,
        protocol::NativeAuthentication,
    )> {
        if !claims.is_object() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "native client data must be a JSON object",
            ));
        }
        let deadline = tokio::time::Instant::now() + ACQUISITION_TIMEOUT;
        let result = tokio::time::timeout_at(deadline, async {
            check_preparation_deadline(deadline)?;
            let mut operation = Arc::clone(&self).acquire_verified().await?;
            check_preparation_deadline(deadline)?;
            operation.promote()?;
            let identity = ReadIdentity::capture(&operation);
            let (prepared, authentication) = self
                .prepare_messaging_socket_borrowed(
                    &mut operation,
                    target.recipient().clone(),
                    claims,
                    random_id,
                    deadline,
                )
                .await?;
            check_preparation_deadline(deadline)?;
            if !identity.current_operation(&operation) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "raw service account changed",
                ));
            }
            let endpoint = target.connection_target();
            let authentication = authentication.with_raw_messaging(prepared, target)?;
            Ok((endpoint, authentication))
        })
        .await
        .map_err(|_| {
            io::Error::new(io::ErrorKind::TimedOut, "raw service preparation timed out")
        })?;
        check_preparation_deadline(deadline)?;
        result
    }
}

#[cfg(test)]
mod tests;
