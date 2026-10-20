//! Explicit friend-world membership, RTA progress and authoritative connection admission.

mod selection;
mod world;
pub use selection::FriendSelection;

use super::directory::{
    Scope, bounded, check_deadline, cleanup_pending, directory_error, event_changed, invalid,
    rta_error, service, socket_error,
};
use super::{NativeAccount, Operation};
use bedrock_net::messaging::Recipient;
use bedrock_services::xbox::{mpsd, rta};
use rta::websocket::{Options, RtaSocket, SocketError};
use sha2::{Digest, Sha256};
use std::{io, sync::Arc, time::Instant};
use uuid::Uuid;

/// A catalog selection retaining its actual service identity and selected account generation.
/// This is an intent, not membership approval; fields and diagnostics never expose its locator.
#[derive(Clone)]
pub struct NativeFriendSelection {
    account: Arc<NativeAccount>,
    scope: Scope,
    selection: FriendSelection,
}

impl NativeFriendSelection {
    /// Captures an existing verified catalog owner without reading credentials or starting I/O.
    pub(super) fn capture(
        account: Arc<NativeAccount>,
        generation: String,
        xuid: String,
        selection: FriendSelection,
    ) -> Self {
        Self {
            account,
            scope: Scope { generation, xuid },
            selection,
        }
    }

    /// Identifies the selected world without exposing its locator or granting membership.
    pub fn world_identity(&self) -> String {
        let reference = &self.selection.reference;
        let mut digest = Sha256::new();
        digest.update(reference.service_config_id().as_bytes());
        for component in [reference.template_name(), reference.name()] {
            // Reference validation excludes control characters, so each separator is unambiguous.
            digest.update([0]);
            digest.update(component.to_ascii_lowercase().as_bytes());
        }
        format!("friend:{:x}", digest.finalize())
    }

    /// Starts this exact selected handle on its original account generation, on the caller's runtime.
    pub fn begin(self, deadline: Instant) -> io::Result<FriendJoin> {
        let mut join = self.account.friend_join(self.selection, deadline)?;
        join.scope = Some(self.scope);
        Ok(join)
    }
}

/// One completed admission, bound to the account generation that joined the directory.
/// No Debug or serialization: the nonce authorizes this player's game login.
pub struct FriendAdmission {
    scope: Scope,
    recipient: Recipient,
    nonce: zeroize::Zeroizing<String>,
}

impl FriendAdmission {
    /// Borrows the joined host's exact signaling route, never an activity's advertised endpoint.
    pub fn recipient(&self) -> &Recipient {
        &self.recipient
    }

    /// Checks the existing leased account immediately before the caller mints a connection credential.
    pub(super) fn validate(&self, operation: &Operation) -> io::Result<()> {
        self.scope.check(operation)
    }

    /// Moves the immutable joined route and nonce to the owned networking handoff.
    /// Keep the originating FriendJoin alive, drive it through play, and explicitly close it.
    pub fn into_parts(mut self) -> (Recipient, String) {
        (self.recipient, std::mem::take(&mut *self.nonce))
    }

    /// Revokes the original admission if its dialect, exact route or player nonce changes.
    fn matches_identity(&self, original: &(Recipient, [u8; 32])) -> bool {
        let same_route = match (&self.recipient, &original.0) {
            (Recipient::Messaging(current), Recipient::Messaging(previous)) => current == previous,
            (Recipient::Legacy(current), Recipient::Legacy(previous)) => current == previous,
            _ => false,
        };
        let nonce: [u8; 32] = Sha256::digest(self.nonce.as_bytes()).into();
        same_route && nonce == original.1
    }
}

/// Observable result of one explicitly driven admission phase.
pub enum JoinProgress {
    /// The host has supplied this joined player's nonce and a supported connection.
    Ready(FriendAdmission),
    /// MPSD explicitly rejected a concurrent write; the same intent may be retried after this instant.
    RetryAt(Instant),
}

/// A single explicitly selected join whose membership remains reachable across canceled futures.
/// No task, runtime or reconnect policy is created. Call advance to admit, drive through play,
/// and close on every termination path; dropping this value performs no remote I/O.
#[must_use = "retain the friend-world owner and explicitly close its remote membership"]
pub struct FriendJoin {
    account: Arc<NativeAccount>,
    selection: FriendSelection,
    scope: Option<Scope>,
    socket: Option<RtaSocket>,
    subscription: Option<rta::SubscriptionId>,
    subscribed: bool,
    session_subscription: Uuid,
    binding: Option<mpsd::Binding>,
    attempt: Option<mpsd::JoinAttempt>,
    session: Option<Box<mpsd::Session>>,
    ambiguous: bool,
    changed: bool,
    active_refresh_deadline: Option<Instant>,
    retry_at: Option<Instant>,
    deadline: Instant,
    admitted: bool,
    prepared: bool,
    admission_identity: Option<(Recipient, [u8; 32])>,
    closing: bool,
    closed: bool,
}

impl NativeAccount {
    /// Creates a caller-owned intent; no authentication, subscription or membership I/O starts here.
    pub fn friend_join(
        self: Arc<Self>,
        selection: FriendSelection,
        deadline: Instant,
    ) -> io::Result<FriendJoin> {
        check_deadline(deadline)?;
        Ok(FriendJoin {
            account: self,
            selection,
            scope: None,
            socket: None,
            subscription: None,
            subscribed: false,
            session_subscription: Uuid::new_v4(),
            binding: None,
            attempt: None,
            session: None,
            ambiguous: false,
            changed: false,
            active_refresh_deadline: None,
            retry_at: None,
            deadline,
            admitted: false,
            prepared: false,
            admission_identity: None,
            closing: false,
            closed: false,
        })
    }
}

impl FriendJoin {
    /// Waits an explicit 412 retry instant while preserving the original deadline and RTA controls.
    pub async fn wait_retry(&mut self, when: Instant) -> io::Result<()> {
        service(
            self.socket
                .as_mut()
                .ok_or_else(|| invalid("friend RTA is unavailable"))?,
            self.subscription
                .ok_or_else(|| invalid("friend subscription is unavailable"))?,
            &self.selection.reference,
            &mut self.changed,
            self.deadline,
            tokio::time::sleep_until(when.into()),
        )
        .await
    }

    /// Drives the live subscription without an idle disconnect deadline.
    /// Once a refresh starts, its absolute service deadline survives cancellation of this future.
    pub async fn drive_active(&mut self) -> io::Result<()> {
        if !self.admitted || self.closing || self.closed {
            return Err(invalid("friend membership is not active"));
        }
        if !self.changed {
            let event = self
                .socket
                .as_mut()
                .ok_or_else(|| invalid("friend RTA is unavailable"))?
                .drive()
                .await
                .map_err(socket_error)?;
            event_changed(
                event,
                self.subscription
                    .ok_or_else(|| invalid("friend subscription is unavailable"))?,
                &self.selection.reference,
                &mut self.changed,
            )?;
        }
        if self.changed {
            let deadline = *self
                .active_refresh_deadline
                .get_or_insert_with(|| Instant::now() + super::ACQUISITION_TIMEOUT);
            while self.changed {
                self.refresh(deadline).await?;
            }
            self.active_refresh_deadline = None;
        }
        self.current_admission()?;
        Ok(())
    }

    /// Stops local I/O before retaining unresolved membership for another runtime's explicit cleanup.
    /// This never claims the remote member was removed and performs no HTTP or account operation.
    pub fn stop_transport(&mut self) {
        self.closing = true;
        if let Some(socket) = &mut self.socket {
            socket.abort();
        }
    }

    /// Prepares this joined world's signed transport on the same verified leased account.
    /// Retain and drive this owner alongside the returned connection, then explicitly close it.
    /// Matching updates during preparation retire the handoff and require an explicit retry.
    pub async fn prepare_friend(
        &mut self,
        claims: serde_json::Value,
        random_id: protocol::ClientRandomId,
    ) -> io::Result<(
        protocol::NativeConnectionTarget,
        protocol::NativeAuthentication,
    )> {
        if !self.admitted || self.prepared || self.closing || self.closed {
            return Err(invalid("friend membership is not active"));
        }
        if !claims.is_object() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "native client data must be a JSON object",
            ));
        }
        let mut operation = self.operation(self.deadline).await?;
        while self.changed {
            self.refresh_owned(&mut operation, self.deadline).await?;
        }
        let admission = self.current_admission()?;
        admission.validate(&operation)?;
        let (recipient, nonce) = admission.into_parts();
        let target = protocol::NativeMessagingTarget::new(recipient, nonce)?;
        let account = Arc::clone(&self.account);
        let prepared = service(
            self.socket
                .as_mut()
                .ok_or_else(|| invalid("friend RTA is unavailable"))?,
            self.subscription
                .ok_or_else(|| invalid("friend subscription is unavailable"))?,
            &self.selection.reference,
            &mut self.changed,
            self.deadline,
            account.prepare_messaging_borrowed(
                &mut operation,
                target,
                claims,
                random_id,
                self.deadline.into(),
            ),
        )
        .await??;
        if self.changed {
            // Its protected socket needs continuous driving; retire it before awaiting another service read.
            drop(prepared);
            while self.changed {
                self.refresh_owned(&mut operation, self.deadline).await?;
            }
            self.current_admission()?.validate(&operation)?;
            operation.persist()?;
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "friend-world metadata changed during preparation; retry",
            ));
        }
        check_deadline(self.deadline)?;
        operation.persist()?;
        check_deadline(self.deadline)?;
        self.prepared = true;
        Ok(prepared)
    }

    /// Establishes active RTA, joins once, then keeps control traffic progressing until host admission.
    /// Cancellation keeps the stable join intent, known session and ambiguity fence in this owner.
    /// A canceled/failed ambiguous PUT is reconciled by the selected reference, never blindly replayed.
    pub async fn advance(&mut self) -> io::Result<JoinProgress> {
        if self.closed || self.closing || self.admitted {
            return Err(invalid("friend join is no longer admitting"));
        }
        check_deadline(self.deadline)?;
        self.update_ambiguity();
        self.connect().await?;
        if let Some(retry_at) = self.retry_at {
            if Instant::now() < retry_at {
                return Ok(JoinProgress::RetryAt(retry_at));
            }
            self.retry_at = None;
        }
        if self.ambiguous && self.session.is_none() {
            self.reconcile(self.deadline).await?;
        }
        if self.session.is_none() {
            let mut operation = self.operation(self.deadline).await?;
            let socket = self
                .socket
                .as_mut()
                .ok_or_else(|| invalid("friend RTA is unavailable"))?;
            let subscription = self
                .subscription
                .ok_or_else(|| invalid("friend subscription is unavailable"))?;
            let binding = socket
                .connection()
                .directory_binding(subscription, self.session_subscription)
                .map_err(rta_error)?;
            self.binding = Some(binding);
            if self.attempt.is_none() {
                self.attempt = Some(
                    operation
                        .minecraft()?
                        .mpsd()
                        .join_attempt(self.selection.handle, binding, mpsd::MemberData::default())
                        .map_err(directory_error)?,
                );
            }
            self.ambiguous = true;
            let result = service(
                socket,
                subscription,
                &self.selection.reference,
                &mut self.changed,
                self.deadline,
                operation
                    .minecraft()?
                    .mpsd()
                    .join(self.attempt.as_mut().expect("prepared join")),
            )
            .await?;
            match result.map_err(directory_error)? {
                mpsd::JoinOutcome::Joined(session) => {
                    self.session = Some(session);
                    self.ambiguous = false;
                }
                mpsd::JoinOutcome::Precondition { retry_after, .. } => {
                    self.ambiguous = false;
                    let retry_at = Instant::now()
                        .checked_add(retry_after)
                        .ok_or_else(|| invalid("friend retry deadline overflow"))?;
                    self.retry_at = Some(retry_at);
                    operation.persist()?;
                    return Ok(JoinProgress::RetryAt(retry_at));
                }
                _ => {
                    self.ambiguous = false;
                    return Err(invalid("friend-world join was rejected"));
                }
            }
            operation.persist()?;
        }
        loop {
            if self.changed {
                self.refresh(self.deadline).await?;
                continue;
            }
            let session = self
                .session
                .as_ref()
                .ok_or_else(|| invalid("friend membership is unavailable"))?;
            if !session.reference().same(&self.selection.reference)
                || session.is_closed()
                || session.needs_sync()
            {
                return Err(invalid("joined friend-world reference is unavailable"));
            }
            self.check_binding(session)?;
            if let Some(admission) = world::admission(
                session.description(),
                self.scope.as_ref().expect("connected scope"),
                &self.selection.host,
            )? {
                let mut operation = self.operation(self.deadline).await?;
                admission.validate(&operation)?;
                operation.persist()?;
                if self.changed {
                    continue;
                }
                check_deadline(self.deadline)?;
                self.admission_identity = Some((
                    admission.recipient.clone(),
                    Sha256::digest(admission.nonce.as_bytes()).into(),
                ));
                self.admitted = true;
                return Ok(JoinProgress::Ready(admission));
            }
            self.notification(self.deadline).await?;
        }
    }

    /// Drives the active subscription and refreshes complete snapshots after matching shoulder taps.
    /// The transport owner polls this alongside game I/O; it must not spawn a detached lifetime task.
    pub async fn drive(&mut self, deadline: Instant) -> io::Result<()> {
        if !self.admitted || self.closing || self.closed {
            return Err(invalid("friend membership is not active"));
        }
        if !self.changed {
            self.notification(deadline).await?;
        }
        while self.changed {
            self.refresh(deadline).await?;
        }
        self.current_admission()?;
        Ok(())
    }

    /// Reconciles an uncertain write and explicitly leaves only this account's membership.
    /// Cancellation keeps cleanup reachable; call close again with a fresh finite cleanup deadline.
    pub async fn close(&mut self, deadline: Instant) -> io::Result<()> {
        if self.closed {
            return Ok(());
        }
        check_deadline(deadline)?;
        self.update_ambiguity();
        self.closing = true;
        if self.ambiguous && self.session.is_none() {
            self.reconcile(deadline).await?;
        }
        if self
            .session
            .as_ref()
            .is_some_and(|session| !session.is_closed())
        {
            let mut operation = self.operation(deadline).await?;
            let session = self.session.as_mut().expect("checked membership");
            if session.needs_sync() || self.changed {
                bounded(deadline, operation.minecraft()?.mpsd().sync(session))
                    .await?
                    .map_err(directory_error)?;
                self.changed = false;
            }
            if !session.is_closed() {
                let binding = self
                    .binding
                    .ok_or_else(|| invalid("friend cleanup binding is unavailable"))?;
                match bounded(
                    deadline,
                    operation.minecraft()?.mpsd().leave_bound(session, binding),
                )
                .await?
                .map_err(directory_error)?
                {
                    mpsd::Update::Closed => {}
                    _ => {
                        return Err(invalid(
                            "friend membership cleanup requires synchronization",
                        ));
                    }
                }
            }
            operation.persist()?;
        }
        if let Some(socket) = &mut self.socket {
            socket.abort();
        }
        self.session = None;
        self.attempt = None;
        self.closed = true;
        Ok(())
    }

    /// Acquires the sole existing account/lease owner and fences every service operation by generation.
    async fn operation(&mut self, deadline: Instant) -> io::Result<Operation> {
        let expected_generation = self.scope.as_ref().map(|scope| scope.generation.clone());
        let acquire =
            Arc::clone(&self.account).acquire_verified_for(expected_generation.as_deref());
        let mut operation = if !self.closing {
            if let (Some(socket), Some(subscription)) = (&mut self.socket, self.subscription) {
                service(
                    socket,
                    subscription,
                    &self.selection.reference,
                    &mut self.changed,
                    deadline,
                    acquire,
                )
                .await??
            } else {
                bounded(deadline, acquire).await??
            }
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
                "friend-world proof owner changed",
            ));
        }
        if !self.closing
            && let Some(expires) = self.selection.expires
            && operation.minecraft()?.service_time() >= expires
        {
            return Err(invalid("friend-world invitation expired"));
        }
        Ok(operation)
    }

    /// Installs the actual protected RTA socket before awaiting the directory subscription result.
    async fn connect(&mut self) -> io::Result<()> {
        if self.socket.is_none() {
            let mut operation = self.operation(self.deadline).await?;
            let scope = Scope::from_operation(&operation)?;
            if scope.xuid == self.selection.host
                || self
                    .selection
                    .recipient
                    .as_deref()
                    .is_some_and(|recipient| recipient != scope.xuid)
            {
                return Err(invalid("friend-world recipient does not match the account"));
            }
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
                ..Options::default()
            };
            let result = bounded(self.deadline, RtaSocket::connect(&mut request, options)).await?;
            if matches!(result, Err(SocketError::UpgradeStatus(401))) {
                operation.minecraft()?.reject_rta(&request);
            }
            self.socket = Some(result.map_err(socket_error)?);
            self.scope = Some(scope);
            operation.persist()?;
        }
        if self.subscription.is_none() {
            let (subscription, _) = self
                .socket
                .as_mut()
                .expect("connected socket")
                .subscribe(rta::Resource::session_directory(), self.deadline)
                .map_err(rta_error)?;
            self.subscription = Some(subscription);
        }
        while !self.subscribed {
            self.notification(self.deadline).await?;
        }
        Ok(())
    }

    /// Recovers possibly applied membership without repeating an ambiguous handle PUT.
    async fn reconcile(&mut self, deadline: Instant) -> io::Result<()> {
        let mut operation = self.operation(deadline).await?;
        let mut directory = operation.minecraft()?.mpsd();
        let read = directory.read(&self.selection.reference);
        let result = if self.closing {
            bounded(deadline, read).await?
        } else {
            service(
                self.socket
                    .as_mut()
                    .ok_or_else(|| invalid("friend RTA is unavailable"))?,
                self.subscription
                    .ok_or_else(|| invalid("friend subscription is unavailable"))?,
                &self.selection.reference,
                &mut self.changed,
                deadline,
                read,
            )
            .await?
        };
        match result {
            Ok(session) => {
                if !session.is_bound(
                    self.binding
                        .ok_or_else(|| invalid("friend join binding is unavailable"))?,
                ) {
                    return Err(cleanup_pending());
                }
                self.session = Some(Box::new(session));
                self.ambiguous = false;
            }
            // A read cannot prove an earlier canceled write will never apply later.
            Err(mpsd::Error::Status(404)) => return Err(cleanup_pending()),
            Err(error) => return Err(directory_error(error)),
        }
        operation.persist()?;
        Ok(())
    }

    /// Refreshes, rather than merging, title metadata while keeping subscription controls progressing.
    async fn refresh(&mut self, deadline: Instant) -> io::Result<()> {
        let mut operation = self.operation(deadline).await?;
        self.refresh_owned(&mut operation, deadline).await?;
        operation.persist()?;
        Ok(())
    }

    /// Refreshes under an already held lease so preparation never reacquires its own account owner.
    async fn refresh_owned(
        &mut self,
        operation: &mut Operation,
        deadline: Instant,
    ) -> io::Result<()> {
        self.scope
            .as_ref()
            .ok_or_else(|| invalid("friend account scope is unavailable"))?
            .check(operation)?;
        // Preserve the dirty fence if this future is canceled before the complete snapshot arrives.
        self.changed = true;
        let mut changed = false;
        let socket = self
            .socket
            .as_mut()
            .ok_or_else(|| invalid("friend RTA is unavailable"))?;
        let subscription = self
            .subscription
            .ok_or_else(|| invalid("friend subscription is unavailable"))?;
        service(
            socket,
            subscription,
            &self.selection.reference,
            &mut changed,
            deadline,
            operation.minecraft()?.mpsd().sync(
                self.session
                    .as_mut()
                    .ok_or_else(|| invalid("friend membership is unavailable"))?,
            ),
        )
        .await?
        .map_err(directory_error)?;
        self.changed = changed;
        Ok(())
    }

    /// Consumes one ordered control event without creating a polling task or accumulating payloads.
    async fn notification(&mut self, deadline: Instant) -> io::Result<()> {
        let event = bounded(
            deadline,
            self.socket
                .as_mut()
                .ok_or_else(|| invalid("friend RTA is unavailable"))?
                .drive(),
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
            self.subscription
                .ok_or_else(|| invalid("friend subscription is unavailable"))?,
            &self.selection.reference,
            &mut self.changed,
        )
    }

    /// Requires the joined local membership to reflect this socket's actual acknowledged RTA binding.
    fn check_binding(&self, session: &mpsd::Session) -> io::Result<()> {
        let socket = self
            .socket
            .as_ref()
            .ok_or_else(|| invalid("friend RTA is unavailable"))?;
        let binding = socket
            .connection()
            .directory_binding(
                self.subscription
                    .ok_or_else(|| invalid("friend subscription is unavailable"))?,
                self.session_subscription,
            )
            .map_err(rta_error)?;
        if !session.is_bound(binding) {
            return Err(invalid(
                "friend membership is not bound to the active subscription",
            ));
        }
        Ok(())
    }

    /// Revalidates the retained member/peer/nonce identity before mint, handoff or continued play.
    fn current_admission(&self) -> io::Result<FriendAdmission> {
        let session = self
            .session
            .as_ref()
            .ok_or_else(|| invalid("friend membership is unavailable"))?;
        self.check_binding(session)?;
        let admission = world::admission(
            session.description(),
            self.scope
                .as_ref()
                .ok_or_else(|| invalid("friend account scope is unavailable"))?,
            &self.selection.host,
        )?
        .ok_or_else(|| invalid("friend-world admission was revoked"))?;
        if !self
            .admission_identity
            .as_ref()
            .is_some_and(|original| admission.matches_identity(original))
        {
            return Err(invalid("friend-world admission identity changed"));
        }
        Ok(admission)
    }

    /// Distinguishes cancellation during credential acquisition from an actually dispatched join write.
    fn update_ambiguity(&mut self) {
        if self.ambiguous {
            self.ambiguous = self
                .attempt
                .as_ref()
                .is_some_and(mpsd::JoinAttempt::needs_reconciliation);
        }
    }
}

#[cfg(test)]
mod tests;
