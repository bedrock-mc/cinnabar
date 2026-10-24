//! Validated selection identity, separate from authoritative membership and transport readiness.

use super::super::world_metadata::{WORLD_TEMPLATE, xuid};
use super::{invalid, world};
use bedrock_services::xbox::{mpsd, social};
use std::{io, time::SystemTime};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;

/// One explicitly selected service handle and its expected host/session identity.
/// Activity properties supply no transport authority; only the later join can do so.
#[derive(Clone)]
pub struct FriendSelection {
    pub(super) handle: Uuid,
    pub(super) reference: mpsd::Reference,
    pub(super) host: String,
    pub(super) recipient: Option<String>,
    pub(super) expires: Option<SystemTime>,
}

impl FriendSelection {
    /// Captures a complete selected activity without interpreting its advertised nonce or peer.
    pub fn activity(activity: &social::Activity) -> io::Result<Self> {
        if activity.handle_type.as_deref() != Some("activity")
            || activity
                .related_info
                .as_ref()
                .is_some_and(|info| info.closed == Some(true))
        {
            return Err(invalid("friend activity is unavailable"));
        }
        let reference = mpsd::Reference::try_from(
            activity
                .session
                .as_ref()
                .ok_or_else(|| invalid("friend activity has no session reference"))?,
        )
        .map_err(|_| invalid("invalid friend activity reference"))?;
        let custom = activity
            .custom_properties
            .as_ref()
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| invalid("friend activity has no world identity"))?;
        let host =
            world::text(custom, "ownerId").ok_or_else(|| invalid("friend activity has no host"))?;
        if activity
            .owner_xuid
            .as_deref()
            .is_some_and(|owner| owner != host)
        {
            return Err(invalid("friend activity owner mismatch"));
        }
        Self::new(
            activity
                .id
                .ok_or_else(|| invalid("friend activity has no handle"))?,
            reference,
            host.to_owned(),
            None,
            None,
        )
    }

    /// Captures a received invitation's actual sender, recipient, locator and original expiration.
    /// Receipt authentication and the user's decision to accept remain the caller's responsibility.
    pub fn invitation(invitation: &mpsd::Invitation, host_xuid: &str) -> io::Result<Self> {
        let reference = mpsd::Reference::try_from(
            invitation
                .reference()
                .ok_or_else(|| invalid("friend invitation has no session reference"))?,
        )
        .map_err(|_| invalid("invalid friend invitation reference"))?;
        if !invitation.sender_xuid.as_deref().is_some_and(xuid) {
            return Err(invalid("friend invitation has no valid sender"));
        }
        let recipient = invitation
            .invited_xuid
            .as_ref()
            .ok_or_else(|| invalid("friend invitation has no recipient"))?
            .clone();
        let expires = OffsetDateTime::parse(
            invitation
                .expiration
                .as_deref()
                .ok_or_else(|| invalid("friend invitation has no expiration"))?,
            &Rfc3339,
        )
        .map_err(|_| invalid("invalid friend invitation expiration"))?;
        // Any admitted participant can invite; the sender need not be the selected world host.
        Self::new(
            invitation.id,
            reference,
            host_xuid.to_owned(),
            Some(recipient),
            Some(expires.into()),
        )
    }

    /// Restricts this owner to the concrete retail friend-world namespace it understands.
    fn new(
        handle: Uuid,
        reference: mpsd::Reference,
        host: String,
        recipient: Option<String>,
        expires: Option<SystemTime>,
    ) -> io::Result<Self> {
        if handle.is_nil()
            || !xuid(&host)
            || recipient.as_deref().is_some_and(|value| !xuid(value))
            || reference.service_config_id() != bedrock_services::minecraft::SERVICE_CONFIG_ID
            || !reference
                .template_name()
                .eq_ignore_ascii_case(WORLD_TEMPLATE)
        {
            return Err(invalid("invalid friend-world selection"));
        }
        Ok(Self {
            handle,
            reference,
            host,
            recipient,
            expires,
        })
    }
}
