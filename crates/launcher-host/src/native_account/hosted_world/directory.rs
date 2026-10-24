//! Explicit native host publication, RTA progress, conditional nonce writes and exact cleanup.

use super::super::{
    Operation, Owner,
    directory::{
        bounded, check_deadline, directory_error, event_changed, invalid, rta_error, service,
        socket_error,
    },
    world_metadata::WORLD_TEMPLATE,
};
use super::{
    approval::{Approvals, NONCE_BYTES},
    *,
};
use bedrock_services::xbox::{mpsd, rta};
use rta::websocket::{Options, RtaSocket, SocketError};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

mod authority;
mod peer_io;
mod recovery;
use authority::Authority;
mod advertised;
use advertised::Advertisement;

struct ApprovalWrite {
    revision: u64,
    document: Value,
}

/// Actual retained remote state; deliberately has no reference back to its reservation or manager.
pub(super) struct HostDirectory {
    account: Arc<NativeAccount>,
    opening: HostOpening,
    scope: Option<Scope>,
    host_name: String,
    advertisement: Option<Advertisement>,
    reference: mpsd::Reference,
    session_subscription: Uuid,
    socket: Option<RtaSocket>,
    subscription: Option<rta::SubscriptionId>,
    subscribed: bool,
    recovery: Option<recovery::Recovery>,
    binding: Option<mpsd::Binding>,
    attempt: Option<mpsd::PublishAttempt>,
    session: Option<mpsd::Session>,
    changed: bool,
    refresh_deadline: Option<Instant>,
    approvals: Option<Approvals>,
    pending: Option<ApprovalWrite>,
    confirmed: Option<ApprovalWrite>,
    deadline: Instant,
    closing: bool,
    closed: bool,
}

impl HostDirectory {
    /// Borrows only this current original running-world identity; no latest-world lookup is performed.
    pub(super) fn opening(&self) -> io::Result<HostOpening> {
        self.current()?;
        Ok(self.opening.clone())
    }

    /// Applies ready RTA changes and complete refreshes before the caller's next admission check.
    pub(super) async fn service_barrier(&mut self, deadline: Option<Instant>) -> io::Result<()> {
        self.peer_io(deadline, std::future::ready(Ok(())), None)
            .await
    }

    /// Acquires only public provider metadata under the original account lease and live RTA lifetime.
    pub(super) async fn login_policy(
        &mut self,
        deadline: Instant,
    ) -> io::Result<super::NativeHostedLoginPolicy> {
        self.service_barrier(Some(deadline)).await?;
        check_deadline(deadline)?;
        let mut operation = self.operation(deadline).await?;
        let acquire = async {
            operation
                .minecraft()?
                .discover()
                .await
                .map_err(super::super::service_error)?;
            let (issuer, clock) = {
                let services = operation.minecraft()?;
                let observed = Instant::now();
                (
                    services
                        .authorization_issuer()
                        .map_err(super::super::service_error)?
                        .to_owned(),
                    protocol::NativeServiceClock::new(services.service_time(), observed),
                )
            };
            let verifier = operation.provider(&issuer)?.peer_verifier().await?;
            Ok::<_, io::Error>((verifier, clock))
        };
        let (verifier, clock) = service(
            self.socket
                .as_mut()
                .ok_or_else(|| invalid("host RTA is unavailable"))?,
            self.subscription
                .ok_or_else(|| invalid("host subscription is unavailable"))?,
            &self.reference,
            &mut self.changed,
            deadline,
            acquire,
        )
        .await??;
        operation.persist()?;
        drop(operation);
        check_deadline(deadline)?;
        self.service_barrier(Some(deadline)).await?;
        Ok(super::NativeHostedLoginPolicy::new(
            verifier,
            clock,
            deadline,
            self.opening.clone(),
            self.scope
                .as_ref()
                .ok_or_else(|| invalid("host account scope is unavailable"))?
                .clone(),
            self.reference.clone(),
        ))
    }

    /// Reuses actual current directory/world/account checks before matching one prepared Login policy.
    pub(super) fn check_login_policy(
        &self,
        policy: &super::NativeHostedLoginPolicy,
    ) -> io::Result<()> {
        self.current()?;
        policy.check(
            &self.opening,
            self.scope
                .as_ref()
                .ok_or_else(|| invalid("host account scope is unavailable"))?,
            &self.reference,
        )
    }

    /// Captures one actual opening and one stable locator without starting service traffic.
    pub(super) fn new(
        account: Arc<NativeAccount>,
        opening: HostOpening,
        deadline: Instant,
    ) -> io::Result<Self> {
        check_deadline(deadline)?;
        opening.validate_hosting()?;
        let scope = Scope::selected(&account)?;
        let reference = mpsd::Reference::new(
            bedrock_services::minecraft::SERVICE_CONFIG_ID,
            WORLD_TEMPLATE.into(),
            Uuid::new_v4().to_string().to_uppercase(),
        )
        .map_err(directory_error)?;
        Ok(Self {
            account,
            opening,
            scope: Some(scope),
            host_name: String::new(),
            advertisement: None,
            reference,
            session_subscription: Uuid::new_v4(),
            socket: None,
            subscription: None,
            subscribed: false,
            recovery: None,
            binding: None,
            attempt: None,
            session: None,
            changed: false,
            refresh_deadline: None,
            approvals: None,
            pending: None,
            confirmed: None,
            deadline,
            closing: false,
            closed: false,
        })
    }

    /// Opens and acknowledges RTA before deriving the original binding or creating a host member.
    pub(super) async fn advance(&mut self) -> io::Result<()> {
        self.active()?;
        check_deadline(self.deadline)?;
        self.connect().await?;
        if self.session.is_none() {
            let mut operation = self.operation(self.deadline).await?;
            if self.attempt.is_none() {
                let binding = self.live_binding()?;
                self.binding = Some(binding);
                let properties = self.properties()?;
                self.attempt = Some(
                    operation
                        .minecraft()?
                        .mpsd()
                        .publish_attempt(
                            &self.reference,
                            mpsd::Publish {
                                binding,
                                member: mpsd::MemberData::default(),
                                constants: None,
                                max_members: self.advertisement.as_ref().and_then(|_| {
                                    std::num::NonZeroU32::new(self.effective_max_players())
                                }),
                                properties: Some(properties),
                                join: if self.advertisement.is_some() {
                                    mpsd::Restriction::Followed
                                } else {
                                    mpsd::Restriction::Local
                                },
                                read: mpsd::Restriction::Followed,
                            },
                        )
                        .map_err(directory_error)?,
                );
            }
            let directory = operation.minecraft()?.mpsd();
            let session = self.publish(directory).await?;
            self.session = Some(session);
            self.changed = true;
            operation.persist()?;
        }
        while self.changed || self.session.as_ref().is_some_and(mpsd::Session::needs_sync) {
            self.refresh(self.deadline).await?;
        }
        self.validate_session()?;
        self.observe()?;
        self.opening.validate_hosting()?;
        Ok(())
    }

    /// Seals the one publication on actual SDK dispatch and reconciles uncertain writes by its fixed locator.
    async fn publish(&mut self, mut directory: mpsd::Directory<'_>) -> io::Result<mpsd::Session> {
        let attempt = self
            .attempt
            .as_mut()
            .ok_or_else(|| invalid("native host publication is unavailable"))?;
        let write = async {
            if attempt.needs_reconciliation() {
                directory.reconcile_publish(attempt).await
            } else {
                directory.publish_retained(attempt).await
            }
        };
        service(
            self.socket
                .as_mut()
                .ok_or_else(|| invalid("host RTA is unavailable"))?,
            self.subscription
                .ok_or_else(|| invalid("host subscription is unavailable"))?,
            &self.reference,
            &mut self.changed,
            self.deadline,
            write,
        )
        .await?
        .map_err(directory_error)
    }

    /// Drives live controls without idle timeout; refresh and recovery share their first cutoff.
    pub(super) async fn drive(&mut self) -> io::Result<()> {
        self.active()?;
        self.changed |= self
            .session
            .as_ref()
            .ok_or_else(|| invalid("host membership is unavailable"))?
            .needs_sync();
        if !self.changed && self.recovery.is_none() {
            if self
                .socket
                .as_ref()
                .is_some_and(|socket| socket.connection().is_connected())
            {
                self.validate_session()?;
            }
            let event = self
                .socket
                .as_mut()
                .ok_or_else(|| invalid("host RTA is unavailable"))?
                .drive()
                .await;
            let result = event.map_err(socket_error).and_then(|event| {
                event_changed(
                    event,
                    self.subscription
                        .ok_or_else(|| invalid("host subscription is unavailable"))?,
                    &self.reference,
                    &mut self.changed,
                )
            });
            if let Err(error) = result {
                self.retain_interruption(error)?;
            }
        }
        if self.changed || self.recovery.is_some() {
            let deadline = self.control_deadline(None);
            self.refresh_control(deadline).await?;
            self.refresh_deadline = None;
        }
        self.active()?;
        self.validate_session()
    }

    /// Recovers the original binding and reconciles uncertain approval writes under one cutoff.
    pub(super) async fn publish_approvals(&mut self, deadline: Instant) -> io::Result<()> {
        self.active()?;
        let deadline = self.control_deadline(Some(deadline));
        loop {
            check_deadline(deadline)?;
            if self.recovery.is_some() {
                self.refresh_control(deadline).await?;
            }
            match self.publish_approvals_once(deadline).await {
                Ok(()) => {
                    self.refresh_deadline = None;
                    return Ok(());
                }
                Err(error) => self.retain_interruption(error)?,
            }
        }
    }

    /// Publishes one desired revision; uncertain writes are refreshed before any further conditional write.
    async fn publish_approvals_once(&mut self, deadline: Instant) -> io::Result<()> {
        self.active()?;
        check_deadline(deadline)?;
        let mut operation = self.operation(deadline).await?;
        while self.changed || self.session.as_ref().is_some_and(mpsd::Session::needs_sync) {
            self.refresh_owned(&mut operation, deadline).await?;
        }
        self.validate_session()?;
        self.observe()?;
        if self.pending.is_none() && self.approvals.as_ref().is_some_and(Approvals::acknowledged) {
            operation.persist()?;
            return Ok(());
        }
        if self.pending.is_some() && self.confirm_pending().is_ok() {
            operation.persist()?;
            return Ok(());
        }
        let approvals = self
            .approvals
            .as_mut()
            .ok_or_else(|| invalid("host approval owner is unavailable"))?;
        for owner in approvals.missing() {
            let mut entropy = zeroize::Zeroizing::new([0; NONCE_BYTES]);
            getrandom::fill(&mut *entropy)
                .map_err(|_| io::Error::other("host nonce entropy failed"))?;
            approvals.issue(&owner, &entropy)?;
        }
        approvals.invalidate();
        let revision = approvals.revision();
        self.pending = Some(ApprovalWrite {
            revision,
            document: self.properties()?,
        });
        let pending = self.pending.as_ref().expect("prepared approval write");
        let result = service(
            self.socket
                .as_mut()
                .ok_or_else(|| invalid("host RTA is unavailable"))?,
            self.subscription
                .ok_or_else(|| invalid("host subscription is unavailable"))?,
            &self.reference,
            &mut self.changed,
            deadline,
            operation.minecraft()?.mpsd().set_properties_bound(
                self.session
                    .as_mut()
                    .ok_or_else(|| invalid("host membership is unavailable"))?,
                self.binding
                    .ok_or_else(|| invalid("host binding is unavailable"))?,
                &pending.document,
            ),
        )
        .await
        .and_then(|result| result.map_err(directory_error));
        let result = match result {
            Ok(result) => result,
            Err(error) => {
                self.changed = true;
                return Err(error);
            }
        };
        match result {
            mpsd::Update::Updated => {
                self.observe()?;
                while self.changed {
                    self.refresh_owned(&mut operation, deadline).await?;
                }
                if self.pending.is_some() {
                    self.confirm_pending()?;
                }
                if !self.approvals.as_ref().is_some_and(Approvals::acknowledged) {
                    return Err(invalid("host approval was not acknowledged"));
                }
                operation.persist()?;
                self.active()?;
                Ok(())
            }
            mpsd::Update::PreconditionFailed => {
                self.pending = None;
                self.changed = true;
                operation.persist()?;
                Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "host approval publication conflicted; synchronize before retry",
                ))
            }
            _ => Err(invalid(
                "host approval publication did not retain membership",
            )),
        }
    }

    /// Captures nonce-policy evidence only after current service acknowledgment and local opening validation.
    pub(super) fn check_approval(&self, owner: &str, nonce: &str) -> io::Result<HostedApproval> {
        self.current()?;
        let approvals = self
            .approvals
            .as_ref()
            .ok_or_else(|| invalid("host approval owner is unavailable"))?;
        if !approvals.accepts(owner, nonce) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "hosted-world nonce was not approved",
            ));
        }
        Ok(HostedApproval {
            opening: self.opening.clone(),
            scope: self.scope.as_ref().expect("current scope").clone(),
            reference: self.reference.clone(),
            xuid: owner.into(),
            member_epoch: approvals
                .member_epoch(owner)
                .ok_or_else(|| invalid("host approval member departed"))?,
            nonce: Sha256::digest(nonce.as_bytes()).into(),
        })
    }

    /// Rejects handles from another opening/account/reference or any revoked membership/nonce revision.
    pub(super) fn recheck_approval(&self, approval: &HostedApproval) -> io::Result<()> {
        self.current()?;
        let scope = self.scope.as_ref().expect("current scope");
        let approvals = self
            .approvals
            .as_ref()
            .ok_or_else(|| invalid("host approval owner is unavailable"))?;
        if !self.opening.same(&approval.opening)
            || scope.generation != approval.scope.generation
            || scope.xuid != approval.scope.xuid
            || !self.reference.same(&approval.reference)
            || !approvals.rechecks(&approval.xuid, approval.member_epoch, &approval.nonce)?
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "hosted-world approval was revoked",
            ));
        }
        Ok(())
    }

    /// Reconciles only this original publication and leaves under its exact original member binding/ETag.
    pub(super) async fn close(&mut self, deadline: Instant) -> io::Result<()> {
        if self.closed {
            return Ok(());
        }
        check_deadline(deadline)?;
        self.closing = true;
        if let Some(approvals) = &mut self.approvals {
            approvals.invalidate();
        }
        if !self.needs_cleanup() {
            self.stop();
            self.attempt = None;
            self.closed = true;
            return Ok(());
        }
        let mut operation = self.operation(deadline).await?;
        if self.session.is_none()
            && self
                .attempt
                .as_ref()
                .is_some_and(mpsd::PublishAttempt::needs_reconciliation)
        {
            let session = bounded(
                deadline,
                operation
                    .minecraft()?
                    .mpsd()
                    .reconcile_publish(self.attempt.as_mut().expect("retained publication")),
            )
            .await?
            .map_err(directory_error)?;
            self.session = Some(session);
        }
        if self
            .recovery
            .as_ref()
            .is_some_and(|recovery| recovery.candidate.is_some())
        {
            self.settle_rebind(&mut operation, deadline).await?;
        }
        if let Some(session) = &mut self.session {
            if !session.is_closed() {
                bounded(deadline, operation.minecraft()?.mpsd().sync(session))
                    .await?
                    .map_err(directory_error)?;
                self.changed = false;
            }
            if !session.is_closed() {
                let result = bounded(
                    deadline,
                    operation.minecraft()?.mpsd().leave_bound(
                        session,
                        self.binding
                            .ok_or_else(|| invalid("host cleanup binding is unavailable"))?,
                    ),
                )
                .await?
                .map_err(directory_error)?;
                if !matches!(result, mpsd::Update::Closed) {
                    return Err(invalid("host cleanup requires synchronization"));
                }
            }
        }
        operation.persist()?;
        self.stop();
        self.session = None;
        self.attempt = None;
        self.closed = true;
        Ok(())
    }

    /// Retains only actual known membership or SDK-proven may-have-applied publication on Drop.
    pub(super) fn needs_cleanup(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(|session| !session.is_closed())
            || self
                .attempt
                .as_ref()
                .is_some_and(mpsd::PublishAttempt::needs_reconciliation)
            || self
                .recovery
                .as_ref()
                .is_some_and(|recovery| recovery.pending())
    }

    /// Stops local I/O and revokes approvals without authenticating, leaving or stopping the world process.
    pub(super) fn stop(&mut self) {
        self.closing = true;
        if let Some(approvals) = &mut self.approvals {
            approvals.invalidate();
        }
        if let Some(socket) = &mut self.socket {
            socket.abort();
        }
    }

    /// Fences every new service operation by the actual opening, generation, XUID and original RTA proof.
    async fn operation(&mut self, deadline: Instant) -> io::Result<Operation> {
        if !self.closing {
            self.opening.validate_hosting()?;
        }
        let generation = self.scope.as_ref().map(|scope| scope.generation.clone());
        let acquire = Arc::clone(&self.account).acquire_verified_for(generation.as_deref());
        let live = self
            .socket
            .as_ref()
            .is_some_and(|socket| socket.connection().is_connected());
        let mut operation = if !self.closing
            && (self.recovery.is_none() || live)
            && let (Some(socket), Some(subscription)) = (&mut self.socket, self.subscription)
        {
            service(
                socket,
                subscription,
                &self.reference,
                &mut self.changed,
                deadline,
                acquire,
            )
            .await??
        } else {
            bounded(deadline, acquire).await??
        };
        if let Some(scope) = &self.scope {
            scope.check(&operation)?;
        }
        operation.promote()?;
        if let Some(socket) = &self.socket
            && !operation.minecraft()?.owns_rta(socket.connection())
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "host RTA proof owner changed",
            ));
        }
        if !self.closing {
            self.opening.validate_hosting()?;
        }
        Ok(operation)
    }

    /// Installs the protected socket before awaiting its real subscription acknowledgment.
    async fn connect(&mut self) -> io::Result<()> {
        if self.socket.is_none() {
            let mut operation = self.operation(self.deadline).await?;
            let scope = Scope::from_operation(&operation)?;
            self.host_name = operation
                .cached
                .as_ref()
                .expect("account owner")
                .identity
                .as_ref()
                .expect("verified identity")
                .gamertag
                .clone();
            self.approvals = Some(Approvals::new(
                scope.xuid.clone(),
                self.effective_max_players(),
            )?);
            self.scope = Some(scope.clone());
            let headers = rta::websocket::upgrade_headers().map_err(socket_error)?;
            let mut request = bounded(
                self.deadline,
                operation
                    .minecraft()?
                    .rta_connection(scope.rta_generation()?, headers),
            )
            .await?
            .map_err(rta_error)?;
            let options = Options {
                connect_timeout: self.deadline.saturating_duration_since(Instant::now()),
                ..Default::default()
            };
            let result = bounded(self.deadline, RtaSocket::connect(&mut request, options)).await?;
            if matches!(result, Err(SocketError::UpgradeStatus(401))) {
                operation.minecraft()?.reject_rta(&request);
            }
            self.socket = Some(result.map_err(socket_error)?);
            operation.persist()?;
        }
        if self.subscription.is_none() {
            let (subscription, _) = self
                .socket
                .as_mut()
                .expect("connected host socket")
                .subscribe(rta::Resource::session_directory(), self.deadline)
                .map_err(rta_error)?;
            self.subscription = Some(subscription);
        }
        while !self.subscribed {
            let event = bounded(
                self.deadline,
                self.socket.as_mut().expect("connected host socket").drive(),
            )
            .await?
            .map_err(socket_error)?;
            if let rta::Event::Subscribed { subscription, .. } = &event
                && Some(*subscription) == self.subscription
            {
                self.subscribed = true;
            }
            event_changed(
                event,
                self.subscription.expect("host subscription"),
                &self.reference,
                &mut self.changed,
            )?;
        }
        self.opening.validate_hosting()?;
        Ok(())
    }

    /// Refreshes under one existing leased owner, retaining dirty state across a canceled HTTP future.
    async fn refresh_owned(
        &mut self,
        operation: &mut Operation,
        deadline: Instant,
    ) -> io::Result<()> {
        self.changed = true;
        if let Some(approvals) = &mut self.approvals {
            approvals.invalidate();
        }
        let mut changed = false;
        service(
            self.socket
                .as_mut()
                .ok_or_else(|| invalid("host RTA is unavailable"))?,
            self.subscription
                .ok_or_else(|| invalid("host subscription is unavailable"))?,
            &self.reference,
            &mut changed,
            deadline,
            operation.minecraft()?.mpsd().sync(
                self.session
                    .as_mut()
                    .ok_or_else(|| invalid("host membership is unavailable"))?,
            ),
        )
        .await?
        .map_err(directory_error)?;
        self.changed = changed;
        self.validate_session()?;
        self.observe()?;
        if !self.changed {
            if self.pending.is_some() {
                let _ = self.confirm_pending();
            } else {
                let _ = self.confirm_retained();
            }
        }
        Ok(())
    }

    /// Borrows the existing account once for a complete snapshot rather than reacquiring a held lease.
    async fn refresh(&mut self, deadline: Instant) -> io::Result<()> {
        let mut operation = self.operation(deadline).await?;
        self.refresh_owned(&mut operation, deadline).await?;
        operation.persist()
    }

    /// Requires the local host member to remain on the actual acknowledged original RTA binding.
    fn validate_session(&self) -> io::Result<()> {
        let session = self
            .session
            .as_ref()
            .ok_or_else(|| invalid("host membership is unavailable"))?;
        if session.is_closed()
            || session.needs_sync()
            || !session.reference().same(&self.reference)
            || !session.is_bound(
                self.binding
                    .ok_or_else(|| invalid("host binding is unavailable"))?,
            )
        {
            return Err(invalid(
                "host original membership was replaced or requires synchronization",
            ));
        }
        if !session.is_bound(self.live_binding()?) {
            return Err(invalid("host live subscription is unavailable"));
        }
        Ok(())
    }

    /// Derives the stable session binding only from the acknowledged original pure RTA connection.
    fn live_binding(&self) -> io::Result<mpsd::Binding> {
        self.socket
            .as_ref()
            .ok_or_else(|| invalid("host RTA is unavailable"))?
            .connection()
            .directory_binding(
                self.subscription
                    .ok_or_else(|| invalid("host subscription is unavailable"))?,
                self.session_subscription,
            )
            .map_err(rta_error)
    }

    /// Reconciles desired nonce epochs from the complete current host/member description.
    fn observe(&mut self) -> io::Result<()> {
        let description = self
            .session
            .as_ref()
            .ok_or_else(|| invalid("host membership is unavailable"))?
            .description();
        self.approvals
            .as_mut()
            .ok_or_else(|| invalid("host approval owner is unavailable"))?
            .observe(description)
    }

    /// Acknowledges only the full exact document and membership revision confirmed by service metadata.
    fn confirm_pending(&mut self) -> io::Result<()> {
        self.active()?;
        self.validate_session()?;
        if self.changed {
            return Err(invalid("host approval snapshot changed"));
        }
        let pending = self
            .pending
            .as_ref()
            .ok_or_else(|| invalid("host approval write is unavailable"))?;
        let actual = self
            .session
            .as_ref()
            .expect("validated membership")
            .description()
            .properties
            .as_ref()
            .and_then(|section| section.custom.as_ref())
            .ok_or_else(|| invalid("host custom metadata is unavailable"))?;
        if actual != &pending.document {
            return Err(invalid("host approval document was not acknowledged"));
        }
        self.approvals
            .as_mut()
            .ok_or_else(|| invalid("host approval owner is unavailable"))?
            .acknowledge(
                pending.revision,
                actual
                    .get("nonces")
                    .ok_or_else(|| invalid("host nonce metadata is unavailable"))?,
            )?;
        self.confirmed = self.pending.take();
        Ok(())
    }

    /// Restores a prior service acknowledgment only after its exact document and member epoch survive refresh.
    fn confirm_retained(&mut self) -> io::Result<()> {
        self.active()?;
        self.validate_session()?;
        if self.changed || self.pending.is_some() {
            return Err(invalid("host approval publication is unresolved"));
        }
        let confirmed = self
            .confirmed
            .as_ref()
            .ok_or_else(|| invalid("host acknowledged approval is unavailable"))?;
        let actual = self
            .session
            .as_ref()
            .expect("validated membership")
            .description()
            .properties
            .as_ref()
            .and_then(|section| section.custom.as_ref())
            .ok_or_else(|| invalid("host custom metadata is unavailable"))?;
        if self
            .approvals
            .as_ref()
            .ok_or_else(|| invalid("host approval owner is unavailable"))?
            .revision()
            != confirmed.revision
            || actual != &confirmed.document
            || actual != &self.properties()?
        {
            return Err(invalid("host acknowledged approval changed"));
        }
        self.approvals
            .as_mut()
            .ok_or_else(|| invalid("host approval owner is unavailable"))?
            .acknowledge(
                confirmed.revision,
                actual
                    .get("nonces")
                    .ok_or_else(|| invalid("host nonce metadata is unavailable"))?,
            )
    }

    /// Produces the complete nonce document, adding a route only for the actual configured acceptor.
    fn properties(&self) -> io::Result<Value> {
        let world = self.opening.world();
        let scope = self
            .scope
            .as_ref()
            .ok_or_else(|| invalid("host account scope is unavailable"))?;
        let nonces = self
            .approvals
            .as_ref()
            .ok_or_else(|| invalid("host approval owner is unavailable"))?
            .document()?;
        let mode = match world.game_mode {
            bridge::GameMode::Creative => "Creative",
            bridge::GameMode::Adventure => "Adventure",
            _ => "Survival",
        };
        let mut properties = json!({"hostName":self.host_name,"ownerId":scope.xuid,"version":protocol::GAME_VERSION,"levelId":world.id,"worldName":world.name,"worldType":mode,"protocol":protocol::PROTOCOL_VERSION,"MemberCount":1,"MaxMemberCount":self.effective_max_players(),"nonces":nonces});
        if let Some(advertisement) = &self.advertisement {
            advertisement.extend(&mut properties);
        }
        Ok(properties)
    }

    /// Rejects use after retirement before starting authentication or accepting any title approval.
    fn active(&self) -> io::Result<()> {
        if self.closing || self.closed {
            return Err(super::closed());
        }
        self.opening.validate_hosting()
    }
}

#[cfg(test)]
pub(in crate::native_account::hosted_world) mod tests;
