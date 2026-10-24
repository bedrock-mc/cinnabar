//! Concrete original authority checks retain their typed continuations through account contention.

use super::*;

impl NativeHostedWorld {
    /// Waits for the same membership proof while continuing original service controls and cutoff.
    pub(in crate::native_account::hosted_world) async fn wait_approval(
        &mut self,
        approval: &HostedApproval,
        deadline: Option<Instant>,
    ) -> io::Result<()> {
        loop {
            self.directory_mut()?.service_barrier(deadline).await?;
            match self.recheck_approval(approval) {
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                result => return result,
            }
        }
    }

    /// Keeps the original handshake policy and finite budget through a temporary SDK checkout.
    pub(in crate::native_account::hosted_world) async fn wait_policy(
        &mut self,
        policy: &NativeHostedLoginPolicy,
    ) -> io::Result<()> {
        loop {
            self.directory_mut()?
                .service_barrier(Some(policy.deadline))
                .await?;
            match self.directory_ref()?.check_login_policy(policy) {
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                result => return result,
            }
        }
    }

    /// Retains key exchange and its same original grant while the actual account is temporarily busy.
    pub(in crate::native_account::hosted_world) async fn wait_login_grant(
        &mut self,
        policy: &NativeHostedLoginPolicy,
        approval: &HostedLoginApproval,
    ) -> io::Result<()> {
        loop {
            self.directory_mut()?
                .service_barrier(Some(approval.deadline))
                .await?;
            match self.recheck_login_grant(policy, approval) {
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                result => return result,
            }
        }
    }

    /// Retains the actual verified Login before its original synchronous approval decision.
    pub(in crate::native_account::hosted_world) async fn wait_approve_login(
        &mut self,
        login: &AuthenticatedLogin,
        policy: &NativeHostedLoginPolicy,
    ) -> io::Result<HostedLoginApproval> {
        loop {
            self.directory_mut()?
                .service_barrier(Some(policy.deadline))
                .await?;
            match self.approve_login(login, policy) {
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                result => return result,
            }
        }
    }

    /// Rechecks the same typed Login and grant after waits, without reacquisition or a new approval.
    pub(in crate::native_account::hosted_world) async fn wait_login(
        &mut self,
        login: &AuthenticatedLogin,
        policy: &NativeHostedLoginPolicy,
        approval: &HostedLoginApproval,
    ) -> io::Result<()> {
        loop {
            self.directory_mut()?
                .service_barrier(Some(approval.deadline))
                .await?;
            match self.recheck_login(login, policy, approval) {
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                result => return result,
            }
        }
    }
}
