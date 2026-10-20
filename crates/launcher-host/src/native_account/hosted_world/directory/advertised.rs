//! Publication and invitation on the original directory after an actual accepting transport is retained.

use super::*;
use crate::native_account::world_metadata::{
    FRIENDS_JOINABILITY, FRIENDS_OF_FRIENDS_BROADCAST, JSON_RPC_SIGNALING, NETHERNET_TRANSPORT,
};
use bedrock_net::messaging::server::MessagingServer;
use bedrock_services::signaling::ConnectionRequest;
use p384::{ecdsa::SigningKey, elliptic_curve::Generate};

/// The route is derived from an actual bound owner, not caller-provided title JSON.
pub(super) struct Advertisement {
    network_id: u64,
    messaging_id: Uuid,
    max_players: u32,
    occupied: usize,
    activity: Activity,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Activity {
    Unpublished,
    Dispatched,
    Acknowledged,
}

impl Advertisement {
    /// Adds the version-matched JSON-RPC world route to the same complete nonce document.
    pub(super) fn extend(&self, properties: &mut Value) {
        properties["MemberCount"] = json!(1 + self.occupied);
        properties["Joinability"] = json!(FRIENDS_JOINABILITY);
        properties["BroadcastSetting"] = json!(FRIENDS_OF_FRIENDS_BROADCAST);
        properties["TransportLayer"] = json!(NETHERNET_TRANSPORT);
        properties["OnlineCrossPlatformGame"] = json!(true);
        properties["rakNetGUID"] = json!("");
        properties["LanGame"] = json!(false);
        properties["isEditorWorld"] = json!(false);
        properties["CrossPlayDisabled"] = json!(false);
        properties["TitleId"] = json!(0);
        properties["SupportedConnections"] = json!([{
            "ConnectionType":JSON_RPC_SIGNALING,
            "NetherNetId":self.network_id,
            "PmsgId":self.messaging_id,
            "HostIpAddress":"",
            "HostPort":0,
        }]);
    }
}

impl HostDirectory {
    /// Reserves the local player in the actual captured process capacity.
    pub(in crate::native_account::hosted_world) fn visitor_limit(&self) -> io::Result<usize> {
        self.active()?;
        let visitors = self.opening.max_players().saturating_sub(1);
        usize::try_from(visitors)
            .ok()
            .filter(|limit| *limit != 0)
            .ok_or_else(|| invalid("native world has no visitor capacity"))
    }
    /// Rejects replacement publication before account or transport acquisition and retains the first cutoff.
    pub(in crate::native_account::hosted_world) fn activation_deadline(
        &mut self,
        deadline: Instant,
    ) -> io::Result<Instant> {
        self.active()?;
        if self.attempt.is_some() || self.session.is_some() || self.advertisement.is_some() {
            return Err(invalid(
                "native accepting route requires an unpublished original world",
            ));
        }
        let deadline = deadline.min(self.deadline);
        check_deadline(deadline)?;
        self.deadline = deadline;
        Ok(deadline)
    }

    /// Changes only actual initialized visitor occupancy, retaining the same nonce membership epoch.
    pub(in crate::native_account::hosted_world) async fn visitor(
        &mut self,
        occupied: usize,
        deadline: Instant,
    ) -> io::Result<()> {
        self.service_barrier(Some(deadline)).await?;
        if self.pending.is_some() {
            return Err(invalid("native host properties are unsettled"));
        }
        let route = self
            .advertisement
            .as_mut()
            .ok_or_else(|| invalid("native accepting route is unavailable"))?;
        if occupied >= route.max_players as usize {
            return Err(invalid(
                "native hosted occupancy exceeds the original capacity",
            ));
        }
        if route.occupied != occupied {
            route.occupied = occupied;
            self.approvals
                .as_mut()
                .ok_or_else(|| invalid("host approval owner is unavailable"))?
                .invalidate();
        }
        Ok(())
    }

    /// Configures only the original unpublished attempt from the actual retained acceptor's transport ID.
    pub(in crate::native_account::hosted_world) fn configure_acceptor(
        &mut self,
        server: &MessagingServer,
        messaging_id: Uuid,
        visitor_limit: usize,
    ) -> io::Result<()> {
        self.active()?;
        if self.attempt.is_some()
            || self.session.is_some()
            || self.advertisement.is_some()
            || messaging_id.is_nil()
            || server.local_network_id() == 0
            || visitor_limit == 0
        {
            return Err(invalid(
                "native accepting route cannot replace an existing publication",
            ));
        }
        let max_players = u32::try_from(visitor_limit)
            .ok()
            .and_then(|visitors| visitors.checked_add(1))
            .ok_or_else(|| invalid("native host capacity is invalid"))?
            .min(self.opening.max_players());
        if max_players < 2 {
            return Err(invalid("native world has no visitor capacity"));
        }
        self.advertisement = Some(Advertisement {
            network_id: server.local_network_id(),
            messaging_id,
            max_players,
            occupied: 0,
            activity: Activity::Unpublished,
        });
        Ok(())
    }

    /// Keeps nonce and advertised membership limits on the same actual configured capacity.
    pub(super) fn effective_max_players(&self) -> u32 {
        self.advertisement
            .as_ref()
            .map_or(self.opening.max_players(), |route| route.max_players)
    }

    /// Mints for the original accepting proof key using the already selected account/provider owner.
    pub(in crate::native_account::hosted_world) async fn hosting_request(
        &mut self,
        deadline: Instant,
    ) -> io::Result<(protocol::NativeHostingIdentity, ConnectionRequest)> {
        self.active()?;
        let mut operation = self.operation(deadline).await?;
        let key = Arc::new(
            p384::ecdsa::SigningKey::try_generate_from_rng(&mut rand::rngs::SysRng)
                .map_err(|_| io::Error::other("native hosting key generation failed"))?,
        );
        let xuid = self
            .scope
            .as_ref()
            .ok_or_else(|| invalid("host account scope is unavailable"))?
            .xuid
            .clone();
        let identity = mint_hosting_identity(&mut operation, key, &xuid, deadline).await?;
        let request = bounded(deadline, operation.minecraft()?.messaging_connection())
            .await?
            .map_err(|_| invalid("native host messaging acquisition failed"))?;
        operation.persist()?;
        self.active()?;
        Ok((identity, request))
    }

    /// Requalifies this host's original key under the same account, membership and live RTA proof.
    /// The caller keeps this finite future pinned while existing peer IO is staged separately.
    pub(in crate::native_account::hosted_world) async fn renew_hosting_identity(
        &mut self,
        original: &protocol::NativeHostingIdentity,
        deadline: Instant,
    ) -> io::Result<protocol::NativeHostingIdentity> {
        self.service_barrier(Some(deadline)).await?;
        let mut operation = self.operation(deadline).await?;
        let xuid = self
            .scope
            .as_ref()
            .ok_or_else(|| invalid("host account scope is unavailable"))?
            .xuid
            .clone();
        let identity = service(
            self.socket
                .as_mut()
                .ok_or_else(|| invalid("host RTA is unavailable"))?,
            self.subscription
                .ok_or_else(|| invalid("host subscription is unavailable"))?,
            &self.reference,
            &mut self.changed,
            deadline,
            mint_hosting_identity(&mut operation, original.signing_key(), &xuid, deadline),
        )
        .await??;
        operation.persist()?;
        drop(operation);
        self.service_barrier(Some(deadline)).await?;
        Ok(identity)
    }

    /// Invalidates only this exact rejected issuance under the original selected account generation.
    pub(in crate::native_account::hosted_world) async fn reject_messaging(
        &mut self,
        request: &ConnectionRequest,
        deadline: Instant,
    ) -> io::Result<()> {
        let mut operation = self.operation(deadline).await?;
        operation.minecraft()?.reject_messaging(request);
        operation.persist()
    }

    /// Requires the original full route/nonce document before one explicit activity publication.
    pub(in crate::native_account::hosted_world) async fn publish_activity(
        &mut self,
        deadline: Instant,
    ) -> io::Result<()> {
        self.service_barrier(Some(deadline)).await?;
        let document = self.properties()?;
        let actual = self
            .session
            .as_ref()
            .expect("current original membership")
            .description()
            .properties
            .as_ref()
            .and_then(|section| section.custom.as_ref());
        let capacity = self.session.as_ref().and_then(|session| {
            session
                .description()
                .constants
                .as_ref()?
                .system
                .as_ref()?
                .get("maxMembersCount")?
                .as_u64()
        });
        if actual != Some(&document) || capacity != Some(u64::from(self.effective_max_players())) {
            return Err(invalid("native hosting route was not acknowledged"));
        }
        let activity = self
            .advertisement
            .as_ref()
            .ok_or_else(|| invalid("native accepting route is unavailable"))?
            .activity;
        match activity {
            Activity::Acknowledged => return Ok(()),
            Activity::Dispatched => return Err(super::super::super::directory::cleanup_pending()),
            Activity::Unpublished => {}
        }
        let mut operation = self.operation(deadline).await?;
        self.advertisement
            .as_mut()
            .expect("actual accepting route")
            .activity = Activity::Dispatched;
        service(
            self.socket
                .as_mut()
                .ok_or_else(|| invalid("host RTA is unavailable"))?,
            self.subscription
                .ok_or_else(|| invalid("host subscription is unavailable"))?,
            &self.reference,
            &mut self.changed,
            deadline,
            operation
                .minecraft()?
                .mpsd()
                .publish_activity(self.session.as_ref().expect("current original membership")),
        )
        .await?
        .map_err(directory_error)?;
        operation.persist()?;
        drop(operation);
        self.service_barrier(Some(deadline)).await?;
        self.advertisement
            .as_mut()
            .expect("actual accepting route")
            .activity = Activity::Acknowledged;
        Ok(())
    }

    /// Dispatches one invitation only from the current actual activity and original membership owner.
    pub(in crate::native_account::hosted_world) async fn invite(
        &mut self,
        xuid: &str,
        deadline: Instant,
    ) -> io::Result<()> {
        self.service_barrier(Some(deadline)).await?;
        if !self
            .advertisement
            .as_ref()
            .is_some_and(|route| route.activity == Activity::Acknowledged)
        {
            return Err(invalid("native hosting activity is unavailable"));
        }
        let mut operation = self.operation(deadline).await?;
        let title = bridge::contract::PRESENCE_TITLE_ID.to_string();
        service(
            self.socket
                .as_mut()
                .ok_or_else(|| invalid("host RTA is unavailable"))?,
            self.subscription
                .ok_or_else(|| invalid("host subscription is unavailable"))?,
            &self.reference,
            &mut self.changed,
            deadline,
            operation.minecraft()?.mpsd().invite(
                self.session.as_ref().expect("current original membership"),
                xuid,
                &title,
            ),
        )
        .await?
        .map_err(directory_error)?;
        operation.persist()?;
        drop(operation);
        self.service_barrier(Some(deadline)).await
    }
}

/// Mints and qualifies one provider proof without acquiring another account or generating a key.
async fn mint_hosting_identity(
    operation: &mut Operation,
    key: Arc<SigningKey>,
    xuid: &str,
    deadline: Instant,
) -> io::Result<protocol::NativeHostingIdentity> {
    let (credential, issuer) = {
        let services = operation.minecraft()?;
        let token = bounded(deadline, services.multiplayer_token(key.verifying_key()))
            .await?
            .map_err(super::super::super::service_error)?;
        let observed = Instant::now();
        (
            protocol::NativeCredential::new(
                token.as_str().to_owned(),
                services.service_time(),
                token.expires_at(),
                observed,
            )?,
            services
                .authorization_issuer()
                .map_err(super::super::super::service_error)?
                .to_owned(),
        )
    };
    bounded(
        deadline,
        operation
            .provider(&issuer)?
            .hosting_identity(key, credential, xuid),
    )
    .await?
}
