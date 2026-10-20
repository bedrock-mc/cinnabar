//! Selected native account ownership and connection preparation on the caller's async runtime.

mod acquisition;
mod catalog;
mod catalog_worker;
mod directory;
mod featured;
mod friend_join;
mod friends;
mod home;
mod hosted_world;
mod messaging;
mod metadata;
mod operation;
mod people;
mod prepare;
pub(crate) mod presence;
mod product_api;
mod profile;
mod raw_nethernet;
mod read_identity;
mod realms;
mod restore;
mod service_route;
#[cfg(feature = "test-support")]
pub mod test_support;
mod world_metadata;
pub use catalog::{CatalogCards, CatalogListing};
pub use catalog_worker::CatalogWorker;
pub use featured::NativeGatheringSelection;
pub(crate) use featured::{FeaturedEntries, FeaturedIdentity};
pub use friend_join::{
    FriendAdmission, FriendJoin, FriendSelection, JoinProgress, NativeFriendSelection,
};
pub use friends::NativeFriendWorld;
pub(crate) use home::NativeHomeScope;
pub use hosted_world::{
    ActiveHostedWorld, ClosingHostedPeer, ClosingHostedWorld, HostedApproval, HostedLoginApproval,
    NativeHostedLoginPolicy, NativeHostedPeer, NativeHostedProcess, NativeHostedWorld,
};
pub(crate) use hosted_world::{HostSlot, HostedInvitation};
pub(crate) use people::NativePeople;
pub(crate) use product_api::{ApiCredential, ApiScope};
pub(crate) use profile::NativeProfile;
pub use realms::is_realm_target;
pub(crate) use realms::membership::{PreparedRealmMembership, RealmMembershipResult};

use bedrock_services::{
    minecraft,
    oauth::{Credentials, MicrosoftAuth},
    xbox,
};
use launcher::accounts::{AccountStore, native::NativeLease};
use p384::{ecdsa::SigningKey, elliptic_curve::Generate};
use rand::rngs::SysRng;
use serde_json::Value;
use std::{
    io,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, UNIX_EPOCH},
};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

pub(super) const ACQUISITION_TIMEOUT: Duration = Duration::from_secs(30);
pub(super) const LEASE_RETRY: Duration = Duration::from_millis(25);
pub(super) const XBOX_RELYING_PARTY: &str = bridge::contract::NATIVE_AUTH_RELYING_PARTY;
pub(super) const REFRESH_ONLY_EXPIRY: &str = "1970-01-01T00:00:00Z";

/// Retains proof and service credentials for one selected application account.
/// No runtime, polling task or HTTP request is created until an operation is polled.
/// Credential values and filesystem locations are deliberately absent from Debug.
pub struct NativeAccount {
    store: AccountStore,
    device_path: PathBuf,
    selected_id: Option<String>,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    busy: bool,
    cached: Option<Cached>,
}

struct Cached {
    generation: String,
    binding: [u8; 32],
    checkpoint_revision: [u8; 32],
    owner: Owner,
    identity: Option<AccountIdentity>,
    provider: Option<protocol::NativeProvider>,
}

/// Retains the generation's account anchor and current OAuth identity validation.
struct AccountIdentity {
    xuid: String,
    gamertag: String,
    current_credential: bool,
}

impl Cached {
    /// Retains the generation's verified XUID while invalidating identity for a changed OAuth revision.
    fn adopt_credentials(&mut self, credentials: Credentials, binding: [u8; 32]) -> io::Result<()> {
        self.owner.replace_credentials(credentials)?;
        self.binding = binding;
        if let Some(identity) = &mut self.identity {
            identity.current_credential = false;
        }
        Ok(())
    }
}

enum Owner {
    Xbox(Box<xbox::Account>),
    Minecraft(Box<minecraft::Minecraft>),
}

/// Takes the SDK owner out of its data mutex for the entire operation.
/// Drop persists an already acquired OAuth rotation, including cancellation during later HTTP.
struct Operation {
    account: Arc<NativeAccount>,
    cached: Option<Cached>,
    lease: Option<NativeLease>,
    published: bool,
    rejected: bool,
    suppressed: bool,
}

impl NativeAccount {
    /// Binds a selected identity to its existing account store without accessing credentials.
    pub fn new(store: AccountStore, device_path: PathBuf, selected_id: Option<String>) -> Self {
        Self {
            store,
            device_path,
            selected_id,
            state: Mutex::new(State::default()),
        }
    }

    /// Reserves the owned SDK value with a short data lock, never retaining that guard during I/O.
    fn begin(self: &Arc<Self>) -> io::Result<Operation> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| io::Error::other("native account owner unavailable"))?;
        if state.busy {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "native account preparation already active",
            ));
        }
        state.busy = true;
        Ok(Operation {
            account: Arc::clone(self),
            cached: state.cached.take(),
            lease: None,
            published: false,
            rejected: false,
            suppressed: false,
        })
    }
}

impl Owner {
    /// Revalidates the current credential's Xbox identity without consuming the Minecraft session.
    async fn xbox_authorization(&mut self) -> io::Result<Arc<xbox::Authorization>> {
        match self {
            Self::Xbox(account) => account.authorize(XBOX_RELYING_PARTY).await,
            Self::Minecraft(services) => services.xbox_authorization().await,
        }
        .map_err(service_error)
    }

    /// Borrows the current rotated OAuth state without cloning the SDK owner.
    fn credentials(&self) -> &Credentials {
        match self {
            Self::Xbox(account) => account.credentials(),
            Self::Minecraft(services) => services.credentials(),
        }
    }

    /// Encodes proof/refresh ownership for protected generation-bound storage.
    fn checkpoint(&self) -> io::Result<xbox::Checkpoint> {
        match self {
            Self::Xbox(account) => account.checkpoint(),
            Self::Minecraft(services) => services.checkpoint(),
        }
        .map_err(service_error)
    }

    /// Replaces only a current same-generation credential, preserving proof and game-session identity.
    fn replace_credentials(&mut self, credentials: Credentials) -> io::Result<()> {
        match self {
            Self::Xbox(account) => account.replace_credentials(credentials),
            Self::Minecraft(services) => services.replace_credentials(credentials),
        }
        .map_err(service_error)
    }
}

/// Chooses the current application's Android policy explicitly, independent of SDK defaults.
pub(super) fn authentication_policy() -> xbox::Config {
    xbox::Config {
        client_id: bridge::contract::NATIVE_AUTH_CLIENT_ID.into(),
        user_agent: bridge::contract::NATIVE_AUTH_USER_AGENT.into(),
        device: xbox::Device::Android,
        device_version: bridge::contract::NATIVE_AUTH_DEVICE_VERSION.into(),
        sandbox: bridge::contract::NATIVE_AUTH_SANDBOX.into(),
    }
}

/// Formats an acquired absolute expiry without panicking on an unrepresentable service value.
pub(super) fn oauth_expiry(expires: std::time::SystemTime) -> io::Result<String> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidInput, "invalid native OAuth expiry");
    let elapsed = expires.duration_since(UNIX_EPOCH).map_err(|_| invalid())?;
    let nanos = i128::try_from(elapsed.as_nanos()).map_err(|_| invalid())?;
    OffsetDateTime::from_unix_timestamp_nanos(nanos)
        .map_err(|_| invalid())?
        .format(&Rfc3339)
        .map_err(|_| invalid())
}

/// Imports the leased refresh credential conservatively; process-local expiry anchors are not portable.
fn import_credentials(lease: &NativeLease, policy: &xbox::Config) -> io::Result<Credentials> {
    Credentials::from_tokens(
        policy.client_id.clone(),
        String::new(),
        lease.refresh_token().to_owned(),
        UNIX_EPOCH,
    )
    .map_err(service_error)
}

/// Keeps HTTP bodies, URLs and credential values out of the application error surface.
fn service_error(error: bedrock_services::Error) -> io::Error {
    match error {
        bedrock_services::Error::SignInRequired => io::Error::other(RenewSignIn),
        bedrock_services::Error::AccountCreationRequired(challenge) => io::Error::other(challenge),
        _ => io::Error::other("native account service acquisition failed; retry or sign in again"),
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Microsoft sign-in must be renewed")]
struct RenewSignIn;

/// Identifies only an explicit OAuth invalid-grant response, without guessing from service failures.
pub(super) fn requires_sign_in(error: &io::Error) -> bool {
    error
        .get_ref()
        .is_some_and(|cause| cause.is::<RenewSignIn>())
}

#[cfg(test)]
mod tests;
