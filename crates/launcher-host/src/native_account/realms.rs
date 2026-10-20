//! Account-borrowed Realms calls and the application's existing menu projection.

use super::service_route::ResolvedServiceRoute;
use super::*;
use bedrock_services::realms::{Join, Realm, RealmAddress};
use launcher::menu::view::MenuRealmCard;
use prepare::check_preparation_deadline;
use std::num::NonZeroU64;

const RESOLUTION_TIMEOUT: Duration = Duration::from_secs(45);

pub(super) mod membership;

impl NativeAccount {
    /// Resolves a selected Realm and mints on the same mutable Minecraft owner, with no Go fallback.
    /// Direct targets retain their existing preparation; the returned target is the only endpoint dialled.
    pub async fn prepare_connection(
        self: Arc<Self>,
        target: protocol::NativeConnectionTarget,
        claims: Value,
        random_id: protocol::ClientRandomId,
    ) -> io::Result<(
        protocol::NativeConnectionTarget,
        protocol::NativeAuthentication,
    )> {
        target.validate()?;
        if target.transport == protocol::NativeTransport::LocalNetherNetHttp {
            target.local_http_address()?;
            let authentication = self.prepare(claims, random_id).await?;
            return Ok((target, authentication));
        }
        if !matches!(
            target.transport,
            protocol::NativeTransport::RakNet
                | protocol::NativeTransport::Addressed
                | protocol::NativeTransport::NetherNetLan
        ) {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "selected native transport does not support account preparation",
            ));
        }
        if let Some(raw) = protocol::NativeRawTarget::parse(&target.address)? {
            return self.prepare_raw_nethernet(raw, claims, random_id).await;
        }
        if let bridge::ConnectTarget::Gathering(id) =
            crate::launcher_core::target_for(&target.address)
        {
            let experience = uuid::Uuid::parse_str(&id)
                .ok()
                .filter(|id| !id.is_nil())
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "invalid Gathering identifier")
                })?;
            return self
                .prepare_gathering(None, experience, claims, random_id)
                .await;
        }
        let Some(selection) = realm_target(&target.address)? else {
            let authentication = self.prepare(claims, random_id).await?;
            return Ok((target, authentication));
        };
        if !claims.is_object() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "native client data must be a JSON object",
            ));
        }
        let deadline = tokio::time::Instant::now() + RESOLUTION_TIMEOUT + ACQUISITION_TIMEOUT;
        let result = tokio::time::timeout_at(deadline, async {
            check_preparation_deadline(deadline)?;
            let mut operation = Arc::clone(&self).acquire_verified().await?;
            check_preparation_deadline(deadline)?;
            operation.promote()?;
            let resolution_deadline =
                deadline.min(tokio::time::Instant::now() + RESOLUTION_TIMEOUT);
            let route = tokio::time::timeout_at(resolution_deadline, async {
                check_preparation_deadline(resolution_deadline)?;
                let mut realms = operation.minecraft()?.realms().await.map_err(realm_error)?;
                check_preparation_deadline(resolution_deadline)?;
                let id = match selection {
                    RealmTarget::Id(id) => id,
                    RealmTarget::Invite(code) => {
                        realms.preview_invite(code).await.map_err(realm_error)?.id
                    }
                };
                loop {
                    check_preparation_deadline(resolution_deadline)?;
                    let joined = realms.join(id, &[]).await.map_err(realm_error)?;
                    check_preparation_deadline(resolution_deadline)?;
                    match joined {
                        Join::Pending { retry_after } => tokio::time::sleep(retry_after).await,
                        Join::Ready(address) => return resolved_route(address),
                        _ => {
                            return Err(io::Error::new(
                                io::ErrorKind::Unsupported,
                                "unsupported Realms join state",
                            ));
                        }
                    }
                }
            })
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "Realm resolution timed out"))??;
            check_preparation_deadline(resolution_deadline)?;
            operation.persist()?;
            check_preparation_deadline(deadline)?;
            let result = route
                .route
                .prepare(&self, &mut operation, claims, random_id, deadline)
                .await;
            drop(route.response);
            result
        })
        .await
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::TimedOut,
                "Realm account preparation timed out",
            )
        })?;
        check_preparation_deadline(deadline)?;
        result
    }
}

enum RealmTarget<'a> {
    Id(NonZeroU64),
    Invite(&'a str),
}

/// Recognizes the existing Realm namespaces without treating them as ordinary server hosts.
pub fn is_realm_target(target: &str) -> bool {
    realm_suffix(target, bridge::contract::REALM_TARGET_PREFIX).is_some()
        || realm_suffix(target, bridge::contract::REALM_CODE_PREFIX).is_some()
}

/// Borrows a namespace's payload using the pinned Go owner's case-insensitive prefix admission.
fn realm_suffix<'a>(target: &'a str, prefix: &str) -> Option<&'a str> {
    let target = target.trim();
    target
        .get(..prefix.len())
        .filter(|value| value.eq_ignore_ascii_case(prefix))?;
    Some(target.get(prefix.len()..)?.trim())
}

/// Parses service-owned target identities without accepting invitations or guessing membership policy.
fn realm_target(target: &str) -> io::Result<Option<RealmTarget<'_>>> {
    if let Some(id) = realm_suffix(target, bridge::contract::REALM_TARGET_PREFIX) {
        return id
            .parse::<NonZeroU64>()
            .map(|id| Some(RealmTarget::Id(id)))
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid Realm identifier"));
    }
    if let Some(code) = realm_suffix(target, bridge::contract::REALM_CODE_PREFIX) {
        if code.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "empty Realm invitation target",
            ));
        }
        return Ok(Some(RealmTarget::Invite(code)));
    }
    Ok(None)
}

/// Projects an actual authenticated service response without consulting a catalog card.
fn resolved_route(response: RealmAddress) -> io::Result<ResolvedRealmRoute> {
    let route =
        ResolvedServiceRoute::new(response.network_protocol.clone(), response.address.clone())?;
    Ok(ResolvedRealmRoute { response, route })
}

/// Keeps the actual Realm metadata until its same-account credential/socket handoff completes.
struct ResolvedRealmRoute {
    response: RealmAddress,
    route: ResolvedServiceRoute,
}

#[cfg(test)]
impl ResolvedRealmRoute {
    /// Consumes the actual validated route in the existing transport projection controls.
    fn into_target(self) -> protocol::NativeConnectionTarget {
        self.route.into_target()
    }
}

/// Projects the pinned Go catalog's fields without resolving addresses or accepting membership.
pub(super) fn card(realm: Realm) -> MenuRealmCard {
    let name = realm.name.trim();
    MenuRealmCard {
        name: if name.is_empty() {
            "Realm".into()
        } else {
            name.into()
        },
        state: realm.state,
        target: format!("{}{}", bridge::contract::REALM_TARGET_PREFIX, realm.id),
        address: String::new(),
        owner: realm.owner.trim().into(),
        online_players: u32::try_from(realm.players.iter().filter(|player| player.online).count())
            .unwrap_or(u32::MAX),
        max_players: realm.max_players,
        days_left: realm.days_left,
        expired: realm.expired,
        member: realm.member,
    }
}

/// Retains actionable access failures without exposing response bodies or service URLs.
pub(super) fn realm_error(error: bedrock_services::realms::Error) -> io::Error {
    match error {
        bedrock_services::realms::Error::NotFound => {
            io::Error::new(io::ErrorKind::NotFound, "Realm was not found")
        }
        bedrock_services::realms::Error::NotMember => io::Error::new(
            io::ErrorKind::PermissionDenied,
            "account cannot access this Realm",
        ),
        _ => io::Error::other("Realms service request failed; retry or sign in again"),
    }
}

#[cfg(test)]
mod tests;
