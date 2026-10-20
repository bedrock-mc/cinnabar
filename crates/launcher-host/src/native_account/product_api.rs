//! The application's product API borrows the selected Minecraft owner for one bound credential.

use super::{NativeAccount, SigningKey, SysRng, read_identity::ReadIdentity, service_error};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use p384::ecdsa::{Signature, signature::Signer};
use p384::elliptic_curve::Generate;
use std::{io, sync::Arc, time::Duration};
use tokio::time::Instant;
use zeroize::Zeroizing;

/// One original generation/XUID, independent of a rejected or expired connection key.
#[derive(Clone)]
pub(crate) struct ApiScope(ReadIdentity);

impl ApiScope {
    /// Distinguishes ordinary account checkout from actual retirement without authenticating.
    pub(crate) fn current(&self) -> Option<bool> {
        self.0.current_status()
    }

    /// The verified account the product API must welcome, never a caller-provided account identity.
    pub(crate) fn xuid(&self) -> &str {
        &self.0.xuid
    }
}

/// Exactly one real SDK-issued token and its P384 proof key; credential contents have no Debug.
pub(crate) struct ApiCredential {
    pub(crate) scope: ApiScope,
    key: SigningKey,
    token: Zeroizing<String>,
    expires: Instant,
}

impl ApiCredential {
    /// Reuses the actual key/token pair until the issuer lifetime approaches the original margin.
    pub(crate) fn reusable(&self, now: Instant) -> bool {
        self.expires.saturating_duration_since(now) > Duration::from_secs(2 * 60)
    }

    /// Borrows only for the original protected hello; no credential clone is needed by the socket owner.
    pub(crate) fn token(&self) -> &str {
        &self.token
    }

    /// Signs the API's exact nonce-bound bytes as raw r||s ES384, not a DER signature.
    pub(crate) fn proof(&self, nonce: &str) -> io::Result<String> {
        if nonce.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "empty API challenge",
            ));
        }
        let signature: Signature = self
            .key
            .try_sign(format!("cinnabar-api/v1/auth:{nonce}").as_bytes())
            .map_err(|_| io::Error::other("product API proof failed"))?;
        Ok(URL_SAFE_NO_PAD.encode(signature.to_bytes()))
    }
}

impl NativeAccount {
    /// Installs actual acquired fixture services under the real saved-account lease and identity owner.
    #[cfg(test)]
    pub(crate) fn product_api_fixture_account(
        root: &std::path::Path,
        services: bedrock_services::minecraft::Minecraft,
        identity: &bedrock_services::xbox::Authorization,
    ) -> (Arc<Self>, launcher::accounts::AccountStore) {
        use super::{Cached, Owner, oauth_expiry};
        use launcher::accounts::{AccountProfile, AccountStore};
        let store = AccountStore::new(root.join("synthetic-oauth.json"));
        let checkpoint = services.checkpoint().unwrap();
        let credentials = services.credentials();
        let pending = store
            .try_begin_native_sign_in("product-fixture-generation", None)
            .unwrap();
        pending
            .try_publish(
                credentials.access_token(),
                credentials.refresh_token(),
                &oauth_expiry(credentials.expires_at()).unwrap(),
            )
            .unwrap();
        pending
            .try_publish_checkpoint(checkpoint.as_bytes())
            .unwrap();
        pending
            .try_commit(
                &AccountProfile::from_identity(identity.xuid(), identity.gamertag()).unwrap(),
            )
            .unwrap();
        let account = Arc::new(Self::new(
            store.clone(),
            root.join("synthetic-device.json"),
            Some(identity.xuid().into()),
        ));
        let mut operation = account.begin().unwrap();
        let lease = store.try_native_lease().unwrap();
        operation.cached = Some(Cached {
            generation: lease.generation().into(),
            binding: lease.binding(),
            checkpoint_revision: lease.checkpoint_revision(),
            owner: Owner::Minecraft(Box::new(services)),
            identity: None,
            provider: None,
        });
        operation.lease = Some(lease);
        operation
            .verify_identity(identity.xuid(), identity.gamertag())
            .unwrap();
        operation.persist().unwrap();
        drop(operation);
        (account, store)
    }

    /// Mints only after the product socket dial, under its caller's one original auth deadline.
    /// Persistence and lease release remain owned by the existing Operation on cancellation.
    pub(crate) async fn product_api_credential(
        self: Arc<Self>,
        scope: Option<&ApiScope>,
    ) -> io::Result<ApiCredential> {
        let mut operation = self
            .acquire_verified_for(scope.map(|scope| scope.0.generation.as_str()))
            .await?;
        if scope.is_some_and(|scope| !scope.0.current_operation(&operation)) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "product API account retired",
            ));
        }
        operation.promote()?;
        let key = SigningKey::try_generate_from_rng(&mut SysRng)
            .map_err(|_| io::Error::other("product API key generation failed"))?;
        let services = operation.minecraft()?;
        let token = services
            .multiplayer_token(key.verifying_key())
            .await
            .map_err(service_error)?;
        let observed = Instant::now();
        let remaining = token
            .expires_at()
            .duration_since(services.service_time())
            .unwrap_or_default();
        let expires = observed
            .checked_add(remaining)
            .ok_or_else(|| io::Error::other("product API credential expiry"))?;
        let token = Zeroizing::new(token.as_str().to_owned());
        let scope = ApiScope(ReadIdentity::capture(&operation));
        operation.persist()?;
        Ok(ApiCredential {
            scope,
            key,
            token,
            expires,
        })
    }
}
