//! Service ownership fixtures without authentication or a second account owner.

use super::NativeAccount;
use std::{io, sync::Arc};

/// Supplies an inert typed intent for frame admission without granting service authority.
pub fn gathering_selection(
    account: Arc<NativeAccount>,
    experience: uuid::Uuid,
) -> super::NativeGatheringSelection {
    super::NativeGatheringSelection {
        identity: super::read_identity::ReadIdentity {
            account,
            generation: String::new(),
            xuid: String::new(),
        },
        experience,
    }
}

/// Holds the actual service checkout and protected lease until the returned closure is dropped.
pub(crate) fn service_operation(
    account: &Arc<NativeAccount>,
) -> io::Result<Box<dyn FnOnce() + Send>> {
    let mut operation = account.begin()?;
    operation.lease = Some(account.store.try_native_lease()?);
    Ok(Box::new(move || drop(operation)))
}
