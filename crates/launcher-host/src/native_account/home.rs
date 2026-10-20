//! Home marketing reads and explicit reports on the selected account's existing SDK owner.

mod projection;

use super::{read_identity::ReadIdentity, *};
use bedrock_services::minecraft::{
    player_messaging,
    profile::{ImageBody, ImageKind},
};
use std::future::Future;
use tokio::time::Instant;
use uuid::Uuid;

/// Binds shown marketing data to its actual account identity and SDK messaging session.
/// This is report metadata, never game membership or connection approval.
#[derive(Clone)]
pub(crate) struct NativeHomeScope {
    identity: ReadIdentity,
    session: Uuid,
    language: String,
}

impl NativeHomeScope {
    /// Uses the actual SDK session to retire optimistic message state when that owner changes.
    pub(crate) fn marker(&self) -> String {
        self.session.to_string()
    }

    /// Compares the original concrete account and service session without inspecting display IDs.
    pub(crate) fn same_session(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.identity.account, &other.identity.account)
            && self.identity.generation == other.identity.generation
            && self.identity.xuid == other.identity.xuid
            && self.session == other.session
            && self.language == other.language
    }

    /// Rechecks a cached presentation off-frame without acquiring a new authorization.
    pub(crate) fn current(&self) -> bool {
        self.identity.current()
    }
}

/// Retains its actual verified lease until the worker finishes cache admission and publication.
pub(crate) struct NativeHome {
    operation: Operation,
    pub(crate) scope: NativeHomeScope,
    invites: Option<u32>,
    treatments: Option<Vec<String>>,
    messages: Option<(Vec<bridge::Message>, bridge::Inbox)>,
    head: Option<ImageBody>,
}

impl NativeHome {
    /// Refills only failed parts; authenticated persona bytes use the existing decoder/cache owner.
    /// The verified Operation remains held after this call through the worker's final snapshot check.
    pub(crate) fn apply(
        &mut self,
        previous: &bridge::Home,
        images: &crate::remote_images::ImageDirectory,
    ) -> Option<(bridge::Home, bool)> {
        if !self.scope.identity.current_operation(&self.operation) {
            return None;
        }
        let mut home = previous.clone();
        let mut any_success = false;
        if let Some(invites) = self.invites.take() {
            home.realm_invites = invites;
            any_success = true;
        }
        if let Some(treatments) = self.treatments.take() {
            home.treatments = treatments;
            any_success = true;
        }
        if let Some((messages, inbox)) = self.messages.take() {
            home.messages = messages;
            home.inbox = inbox;
            any_success = true;
        }
        if let Some(path) = self
            .head
            .take()
            .and_then(|head| images.store_service_image(&head.bytes))
        {
            home.persona_head = bridge::Artwork {
                path: path.to_string_lossy().into_owned(),
                ..Default::default()
            };
            any_success = true;
        }
        Some((home, any_success))
    }
}

/// One exact prepared reporting owner; cancellation before run cannot dispatch an event.
pub(crate) struct PreparedHomeReport {
    operation: Operation,
    language: String,
}

impl PreparedHomeReport {
    /// Sends the actual event once; the worker retains this possible write through its original cutoff.
    pub(crate) async fn run(mut self, event: &bridge::MessageEvent) -> io::Result<()> {
        let result = self
            .operation
            .minecraft()?
            .player_messaging()
            .report_events(
                &self.language,
                &[player_messaging::Event {
                    event_type: &event.event_type,
                    instance_id: &event.instance_id,
                    report_id: &event.report_id,
                    button_id: &event.button_id,
                    time: None,
                }],
            )
            .await;
        self.operation.persist()?;
        result.map_err(service_error)
    }
}

impl NativeAccount {
    /// Reads the existing independent Home parts on one verified proof under the caller's cutoff.
    pub(crate) async fn read_home(
        self: Arc<Self>,
        deadline: Instant,
        language: String,
    ) -> io::Result<NativeHome> {
        let mut operation = tokio::time::timeout_at(deadline, self.acquire_verified())
            .await
            .map_err(|_| timed_out())??;
        if Instant::now() >= deadline {
            return Err(timed_out());
        }
        operation.promote()?;
        let identity = ReadIdentity::capture(&operation);
        let session = operation.minecraft()?.player_messaging().session_id();
        let invites = part(deadline, async {
            operation
                .minecraft()?
                .realms()
                .await
                .map_err(realms::realm_error)?
                .pending_invites()
                .await
                .map_err(realms::realm_error)
        })
        .await?;
        operation.persist()?;
        let treatments = part(deadline, async {
            operation
                .minecraft()?
                .authorize()
                .await
                .map(|token| token.treatments().to_vec())
                .map_err(service_error)
        })
        .await?;
        operation.persist()?;
        let messages = part(deadline, async {
            let session = operation
                .minecraft()?
                .player_messaging()
                .refresh(&language)
                .await
                .map_err(service_error)?;
            projection::flatten(session)
        })
        .await?;
        operation.persist()?;
        let head = part(deadline, async {
            operation
                .minecraft()?
                .profile()
                .image(&identity.xuid, ImageKind::Head)
                .await
                .map_err(service_error)
        })
        .await?;
        operation.persist()?;
        Ok(NativeHome {
            scope: NativeHomeScope {
                identity,
                session,
                language,
            },
            operation,
            invites,
            treatments,
            messages,
            head,
        })
    }

    /// Acquires only the originally shown account/session before the worker commits reporting intent.
    pub(crate) async fn prepare_home_report(
        self: Arc<Self>,
        scope: &NativeHomeScope,
    ) -> io::Result<Option<PreparedHomeReport>> {
        if !Arc::ptr_eq(&self, &scope.identity.account) {
            return Ok(None);
        }
        let mut operation = self
            .acquire_verified_for(Some(&scope.identity.generation))
            .await?;
        operation.promote()?;
        if !scope.identity.current_operation(&operation)
            || operation.minecraft()?.player_messaging().session_id() != scope.session
        {
            return Ok(None);
        }
        Ok(Some(PreparedHomeReport {
            operation,
            language: scope.language.clone(),
        }))
    }
}

/// Keeps completed independent parts when another optional service fails or runs out of time.
async fn part<T>(
    deadline: Instant,
    phase: impl Future<Output = io::Result<T>>,
) -> io::Result<Option<T>> {
    if Instant::now() >= deadline {
        return Ok(None);
    }
    match tokio::time::timeout_at(deadline, phase).await {
        Ok(Ok(value)) if Instant::now() < deadline => Ok(Some(value)),
        Ok(Err(error)) if requires_sign_in(&error) => Err(error),
        _ => Ok(None),
    }
}

/// Keeps account/service diagnostics out of the menu's retry copy.
fn timed_out() -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, "native Home refresh timed out")
}

#[cfg(test)]
mod tests;
