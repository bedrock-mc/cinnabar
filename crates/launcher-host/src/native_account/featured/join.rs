//! Explicit experience allocation, separate from catalog presentation and population reads.

use super::*;
use crate::native_account::service_route::{ResolvedServiceRoute, host_port};
use bedrock_services::realms::NetworkProtocol;

impl std::fmt::Debug for NativeGatheringSelection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativeGatheringSelection")
            .finish_non_exhaustive()
    }
}

impl NativeGatheringSelection {
    /// Checks the concrete UI-selected owner without acquiring credentials on a frame.
    pub fn belongs_to(&self, account: &Arc<NativeAccount>) -> bool {
        Arc::ptr_eq(&self.identity.account, account)
    }

    /// Returns only the selected service namespace; it is never dialled as a hostname.
    pub fn connection_target(&self) -> protocol::NativeConnectionTarget {
        protocol::NativeConnectionTarget::new(
            format!(
                "{}{}",
                bridge::contract::GATHERING_TARGET_PREFIX,
                self.experience
            ),
            protocol::NativeTransport::RakNet,
        )
    }

    /// Allocates once on this original account, then mints for only the actual returned route.
    pub async fn prepare(
        self,
        claims: Value,
        random_id: protocol::ClientRandomId,
    ) -> io::Result<(
        protocol::NativeConnectionTarget,
        protocol::NativeAuthentication,
    )> {
        Arc::clone(&self.identity.account)
            .prepare_gathering(Some(self.identity), self.experience, claims, random_id)
            .await
    }
}

impl NativeAccount {
    /// One explicit selected or CLI experience intent; cancellation never replays its POST.
    pub(in crate::native_account) async fn prepare_gathering(
        self: Arc<Self>,
        identity: Option<ReadIdentity>,
        experience: Uuid,
        claims: Value,
        random_id: protocol::ClientRandomId,
    ) -> io::Result<(
        protocol::NativeConnectionTarget,
        protocol::NativeAuthentication,
    )> {
        if experience.is_nil() || !claims.is_object() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid Gathering join intent",
            ));
        }
        let deadline = Instant::now() + protocol::NATIVE_JOIN_TIMEOUT;
        let result = tokio::time::timeout_at(deadline, async {
            check_preparation_deadline(deadline)?;
            let generation = identity
                .as_ref()
                .map(|identity| identity.generation.as_str());
            let mut operation = Arc::clone(&self).acquire_verified_for(generation).await?;
            check_preparation_deadline(deadline)?;
            operation.promote()?;
            let identity = identity.unwrap_or_else(|| ReadIdentity::capture(&operation));
            if !identity.current_operation(&operation) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "Gathering account changed",
                ));
            }
            let mut gatherings = operation
                .minecraft()?
                .gatherings()
                .await
                .map_err(service_error)?;
            check_preparation_deadline(deadline)?;
            let response = gatherings.join_experience(experience).await;
            operation.persist()?;
            check_preparation_deadline(deadline)?;
            if !identity.current_operation(&operation) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "Gathering account changed",
                ));
            }
            let response = response.map_err(service_error)?;
            let address = match &response.network_protocol {
                NetworkProtocol::Default => host_port(response.ip_v4_address.trim(), response.port),
                _ => response.nethernet_id.trim().to_owned(),
            };
            let route = ResolvedServiceRoute::new(response.network_protocol.clone(), address)?;
            let result = route
                .prepare(&self, &mut operation, claims, random_id, deadline)
                .await;
            check_preparation_deadline(deadline)?;
            if !identity.current_operation(&operation) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "Gathering account changed",
                ));
            }
            // Keep original destination metadata alive until credential/socket preparation completes.
            drop(response);
            result
        })
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "Gathering preparation timed out"))?;
        check_preparation_deadline(deadline)?;
        result
    }
}
