//! Provider-qualified Login bound to the original acknowledged hosted-world approval.

mod wait;

use super::*;
use bedrock_net::login::{AuthenticatedLogin, LoginError, LoginVerifier, TrustedIdentity};
use p384::ecdsa::VerifyingKey;
use protocol::{NativeServiceClock, NativeTokenVerifier};
use sha2::{Digest, Sha256};
use std::time::SystemTime;

/// One host's immutable public verification snapshot and original service-clock/deadline observation.
/// This grants no world access, starts no worker and owns no authentication cache or proof key.
pub struct NativeHostedLoginPolicy {
    verifier: NativeTokenVerifier,
    clock: NativeServiceClock,
    deadline: Instant,
    opening: HostOpening,
    scope: Scope,
    reference: mpsd::Reference,
}

impl NativeHostedLoginPolicy {
    /// Captures only the original directory/account/world and existing public provider owner.
    pub(super) fn new(
        verifier: NativeTokenVerifier,
        clock: NativeServiceClock,
        deadline: Instant,
        opening: HostOpening,
        scope: Scope,
        reference: mpsd::Reference,
    ) -> Self {
        Self {
            verifier,
            clock,
            deadline,
            opening,
            scope,
            reference,
        }
    }

    /// Returns the original absolute deadline to use for this accepting handshake.
    pub fn expires_at(&self) -> Instant {
        self.deadline
    }

    /// Rejects policy transfer to another account generation, directory or actual running world opening.
    pub(super) fn check(
        &self,
        opening: &HostOpening,
        scope: &Scope,
        reference: &mpsd::Reference,
    ) -> io::Result<()> {
        super::super::directory::check_deadline(self.deadline)?;
        if !self.opening.same(opening)
            || self.scope.generation != scope.generation
            || self.scope.xuid != scope.xuid
            || !self.reference.same(reference)
        {
            return Err(denied());
        }
        Ok(())
    }

    /// Samples only the application edge clock, retaining the original calibrated observation and budget.
    pub(super) fn utc(&self) -> Result<SystemTime, LoginError> {
        let now = Instant::now();
        if now >= self.deadline {
            return Err(LoginError::Authentication);
        }
        self.clock.now(now).map_err(|_| LoginError::InvalidTime)
    }

    /// Revalidates one exact Login without fabricating generic host UTC for its provider owner.
    fn revalidate(&self, login: &AuthenticatedLogin) -> io::Result<()> {
        let utc = self.utc().map_err(|_| denied())?;
        login
            .revalidate(&self.verifier, utc)
            .map_err(|_| denied())?;
        super::super::directory::check_deadline(self.deadline)
    }
}

impl LoginVerifier for NativeHostedLoginPolicy {
    /// Uses the original SDK-calibrated clock; generic host UTC cannot extend credential admission.
    fn verify(&self, token: &str, _utc: SystemTime) -> Result<TrustedIdentity, LoginError> {
        let identity = LoginVerifier::verify(&self.verifier, token, self.utc()?)?;
        self.utc()?;
        Ok(identity)
    }
}

/// Current hosted membership approval for one verified Login proof and original nonce.
/// No Debug, Clone or serialization exposes its world/account/member identity.
pub struct HostedLoginApproval {
    pub(super) approval: HostedApproval,
    public_key: VerifyingKey,
    deadline: Instant,
}

impl NativeHostedWorld {
    /// Rechecks the retained caller grant where a key-exchange continuation hides its Login borrow.
    pub(super) fn recheck_login_grant(
        &self,
        policy: &NativeHostedLoginPolicy,
        approval: &HostedLoginApproval,
    ) -> io::Result<()> {
        super::super::directory::check_deadline(approval.deadline)?;
        self.directory_ref()?.check_login_policy(policy)?;
        self.recheck_approval(&approval.approval)
    }
    /// Prepares public Login verification through this host's existing selected account/provider owner.
    ///
    /// Keep this actual future pinned while polling unrelated work. Existing RTA
    /// control is driven during discovery/metadata acquisition; no player token,
    /// proof key, listener or task is minted. A dirty directory must be refreshed
    /// and its nonce policy acknowledged before this method can succeed.
    ///
    /// # Errors
    /// Rejects changed/unacknowledged membership, world/account retirement,
    /// expired budget and bounded discovery/provider acquisition or persistence failure.
    pub async fn login_policy(&mut self, deadline: Instant) -> io::Result<NativeHostedLoginPolicy> {
        self.directory_mut()?.login_policy(deadline).await
    }

    /// Checks the crypto-authenticated Login at the reusable server handshake's application pause.
    ///
    /// Pass this same policy to the accepting handshake. Retain this returned
    /// approval across preparation and recheck it before pack/world dispatch.
    /// Neither a service invitation/card nor client metadata alone authorizes access.
    ///
    /// # Errors
    /// Rejects stale policy/world/account, expired or changed credentials and an unapproved original nonce.
    /// WouldBlock requires retaining the original verified Login while another account operation finishes.
    pub fn approve_login(
        &self,
        login: &AuthenticatedLogin,
        policy: &NativeHostedLoginPolicy,
    ) -> io::Result<HostedLoginApproval> {
        self.directory_ref()?.check_login_policy(policy)?;
        policy.revalidate(login)?;
        let approval = self.check_approval(login.xuid(), login.nonce())?;
        Ok(HostedLoginApproval {
            approval,
            public_key: *login.public_key(),
            deadline: policy.deadline,
        })
    }

    /// Rechecks the exact Login, acknowledged member epoch and original world/account after any await.
    ///
    /// Call immediately before actual pack/world handoff, including after
    /// fallible preparation and transport capacity waits. This performs no I/O;
    /// the accepting runtime/pack publisher/world bridge remain separate owners.
    ///
    /// # Errors
    /// Rejects any changed Login proof/identity/nonce, expired original budget,
    /// revoked membership/approval or retired/replaced world/account binding.
    /// WouldBlock denies new admission during temporary account checkout; retain the same typed owner.
    pub fn recheck_login(
        &self,
        login: &AuthenticatedLogin,
        policy: &NativeHostedLoginPolicy,
        approval: &HostedLoginApproval,
    ) -> io::Result<()> {
        super::super::directory::check_deadline(approval.deadline)?;
        self.directory_ref()?.check_login_policy(policy)?;
        policy.revalidate(login)?;
        if login.xuid() != approval.approval.xuid
            || *login.public_key() != approval.public_key
            || <[u8; 32]>::from(Sha256::digest(login.nonce().as_bytes())) != approval.approval.nonce
        {
            return Err(denied());
        }
        self.recheck_approval(&approval.approval)?;
        super::super::directory::check_deadline(approval.deadline)
    }
}

/// Keeps authenticated account, proof and nonce details out of caller diagnostics.
fn denied() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "native hosted Login approval failed",
    )
}
