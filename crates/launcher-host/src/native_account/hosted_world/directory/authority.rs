//! Original host account/proof authority, distinct from temporary SDK checkout.

use super::*;

pub(super) enum Authority {
    Current,
    Busy,
}

impl HostDirectory {
    /// Checks current service, original world/account and actual RTA proof without credential or file IO.
    pub(super) fn current(&self) -> io::Result<()> {
        self.active()?;
        self.validate_session()?;
        if self.changed || self.pending.is_some() {
            return Err(invalid("host approval publication is unresolved"));
        }
        match self.account_authority()? {
            Authority::Current => Ok(()),
            Authority::Busy => Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "host account operation is active",
            )),
        }
    }
    /// Checks the exact original cached Scope and actual RTA proof only after its SDK owner is available.
    pub(super) fn account_authority(&self) -> io::Result<Authority> {
        let scope = self
            .scope
            .as_ref()
            .ok_or_else(|| invalid("host account scope is unavailable"))?;
        let state = match self.account.available_state() {
            crate::native_account::read_identity::AccountAvailability::Available(state) => state,
            crate::native_account::read_identity::AccountAvailability::Busy => {
                return Ok(Authority::Busy);
            }
            crate::native_account::read_identity::AccountAvailability::Unavailable => {
                return Err(invalid("host account owner is unavailable"));
            }
        };
        let cached = state
            .cached
            .as_ref()
            .ok_or_else(|| invalid("host account owner is unavailable"))?;
        if cached.generation != scope.generation
            || cached
                .identity
                .as_ref()
                .is_none_or(|identity| !identity.current_credential || identity.xuid != scope.xuid)
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "host account generation changed",
            ));
        }
        let socket = self
            .socket
            .as_ref()
            .ok_or_else(|| invalid("host RTA is unavailable"))?;
        if !matches!(&cached.owner, Owner::Minecraft(account) if account.owns_rta(socket.connection()))
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "host RTA proof owner changed",
            ));
        }
        Ok(Authority::Current)
    }
}
