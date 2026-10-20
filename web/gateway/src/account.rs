//! One memory-only SDK account; the control owner retains and cancels its actual futures.

use bedrock_services::{
    minecraft,
    oauth::{MicrosoftAuth, Poll},
    xbox,
};
use bridge::{Account as Observation, AuthState, contract::*};
use p384::{ecdsa::SigningKey, elliptic_curve::Generate};
use protocol::{
    ClientDataRefusal, NativeAuthentication, NativeCredential, NativeDeviceProfile, NativeProvider,
    RawClientData, ValidatedClientData,
};
use rand::rngs::SysRng;
use std::{
    io,
    time::{Instant, SystemTime},
};
use tokio::sync::OwnedSemaphorePermit;

/// Retains the acquired SDK proof, current OAuth state and one ephemeral device profile.
/// No file, protected lease, checkpoint or credential copy exists in this owner.
pub(super) struct Account {
    services: minecraft::Minecraft,
    xuid: String,
    gamertag: String,
    profile: NativeDeviceProfile,
    provider: Option<NativeProvider>,
    // Struct fields drop in order: derived credentials retire before capacity is released.
    _capacity: OwnedSemaphorePermit,
}

impl Account {
    /// Drives one original session-scoped attempt while the service owner handles cancellation.
    pub(super) async fn sign_in(
        deadline: Instant,
        mut publish: impl FnMut(Observation) -> io::Result<()>,
        capacity: OwnedSemaphorePermit,
    ) -> io::Result<Self> {
        tokio::time::timeout_at(deadline.into(), async {
            current(deadline)?;
            let mut auth = MicrosoftAuth::new(authentication_policy()).map_err(failed)?;
            let mut attempt = auth.start(SystemTime::now()).await.map_err(failed)?;
            current(deadline)?;
            publish(Observation {
                state: AuthState::AwaitingCode,
                verification_uri: Some(attempt.verification_uri().as_str().into()),
                user_code: Some(attempt.user_code().into()),
                gamertag: None,
                reason: None,
            })?;
            let credentials = loop {
                let wait = attempt
                    .next_poll()
                    .duration_since(SystemTime::now())
                    .unwrap_or_default();
                tokio::time::sleep(wait).await;
                current(deadline)?;
                match auth
                    .poll(&mut attempt, SystemTime::now())
                    .await
                    .map_err(failed)?
                {
                    Poll::Pending => {}
                    Poll::Authorized(credentials) => break credentials,
                }
            };
            current(deadline)?;
            let mut xbox = auth.into_account(credentials).map_err(failed)?;
            let identity = xbox
                .authorize(NATIVE_AUTH_RELYING_PARTY)
                .await
                .map_err(failed)?;
            current(deadline)?;
            let xuid = identity.xuid().to_owned();
            let gamertag = identity.gamertag().to_owned();
            if xuid.is_empty() || xuid.parse::<i64>().is_err() || gamertag.is_empty() {
                return Err(failed(()));
            }
            let version = u32::try_from(protocol::PROTOCOL_VERSION).map_err(failed)?;
            let config = minecraft::Config::desktop(
                protocol::GAME_VERSION,
                version,
                minecraft::DeviceId::compact(NativeDeviceProfile::service_device_id(&xuid)?),
            );
            Ok(Self {
                _capacity: capacity,
                services: minecraft::Minecraft::new(xbox, config).map_err(failed)?,
                xuid,
                gamertag,
                profile: NativeDeviceProfile::fresh(),
                provider: None,
            })
        })
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "gateway sign-in timed out"))?
    }

    /// Borrows only public account presentation for the bounded control observation.
    pub(super) fn observation(&self) -> Observation {
        Observation {
            state: AuthState::SignedIn,
            gamertag: Some(self.gamertag.clone()),
            verification_uri: None,
            user_code: None,
            reason: None,
        }
    }

    /// Applies this actual account's owned profile without dispatching credential or transport work.
    pub(super) fn validate(
        &self,
        claims: RawClientData,
        protocol: i32,
    ) -> Result<ValidatedClientData, ClientDataRefusal> {
        self.profile.validate_relay(claims, protocol)
    }

    /// Mints on this same SDK account under the caller's original play admission deadline.
    pub(super) async fn prepare(
        &mut self,
        claims: ValidatedClientData,
        deadline: Instant,
    ) -> io::Result<NativeAuthentication> {
        tokio::time::timeout_at(deadline.into(), async {
            current(deadline)?;
            let claims = claims.into_relay_claims(&self.gamertag)?;
            let key = SigningKey::try_generate_from_rng(&mut SysRng).map_err(failed)?;
            let token = self
                .services
                .multiplayer_token(key.verifying_key())
                .await
                .map_err(failed)?;
            current(deadline)?;
            let observed = Instant::now();
            let credential = NativeCredential::new(
                token.as_str().to_owned(),
                self.services.service_time(),
                token.expires_at(),
                observed,
            )?;
            let issuer = self.services.authorization_issuer().map_err(failed)?;
            let same = self
                .provider
                .as_ref()
                .map(|provider| provider.matches(issuer))
                .transpose()?
                .unwrap_or(false);
            if !same {
                self.provider = Some(NativeProvider::new(issuer, minecraft::USER_AGENT)?);
            }
            let authentication = self
                .provider
                .as_mut()
                .expect("selected provider")
                .authentication_relay(key, credential, claims, &self.xuid)
                .await?;
            current(deadline)?;
            Ok(authentication)
        })
        .await
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::TimedOut,
                "gateway account preparation timed out",
            )
        })?
    }
}

/// Applies the application's shared Android policy rather than SDK defaults.
fn authentication_policy() -> xbox::Config {
    xbox::Config {
        client_id: NATIVE_AUTH_CLIENT_ID.into(),
        user_agent: NATIVE_AUTH_USER_AGENT.into(),
        device: xbox::Device::Android,
        device_version: NATIVE_AUTH_DEVICE_VERSION.into(),
        sandbox: NATIVE_AUTH_SANDBOX.into(),
    }
}

/// Drops provider/account/HTTP diagnostics at the browser application boundary.
fn failed<T>(_error: T) -> io::Error {
    io::Error::other("gateway account operation failed")
}

/// Checks the caller's original monotonic budget before dispatch and publication.
fn current(deadline: Instant) -> io::Result<()> {
    if Instant::now() >= deadline {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "gateway account operation timed out",
        ));
    }
    Ok(())
}
