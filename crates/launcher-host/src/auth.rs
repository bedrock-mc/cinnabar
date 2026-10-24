//! Native authentication presentation and the existing installed cache location.

use launcher::install_layout::InstallLayout;
use std::path::PathBuf;

/// The existing in-place OAuth cache when present; no account data is migrated or copied.
pub fn auth_cache_path(layout: &InstallLayout) -> Option<PathBuf> {
    Some(layout.auth_cache()).filter(|path| path.is_file())
}

#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
mod native;

#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
use launcher::menu::auth::AuthState;
#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
use url::Url;

#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
const MAX_LINE_BYTES: usize = 4096;
#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
const EVENT_CAPACITY: usize = 8;

/// Owns the original native attempt and its cleanup receipt independently of presentation.
#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
#[derive(Debug)]
pub struct AuthSupervisor {
    native: native::NativeAuth,
}

#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
impl AuthSupervisor {
    /// Starts native sign-in on the application's existing reactor and tracked worker lifetime.
    pub fn spawn_native(
        store: launcher::accounts::AccountStore,
        device: PathBuf,
        selected: Option<String>,
        adding: bool,
        runtime: tokio::runtime::Handle,
        tasks: tokio_util::task::TaskTracker,
    ) -> anyhow::Result<Self> {
        let native = native::NativeAuth::spawn(store, device, selected, adding, runtime, tasks)?;
        Ok(Self::from_native(native))
    }

    /// Retains the actual native worker until its original cleanup completes.
    fn from_native(native: native::NativeAuth) -> Self {
        Self { native }
    }

    /// Moves verified metadata and its pending transaction to the existing account mutation owner.
    pub fn take_native_profile(
        &mut self,
    ) -> Option<(
        launcher::accounts::AccountProfile,
        Option<launcher::accounts::pending::PendingSignIn>,
    )> {
        self.native.take_profile()
    }

    /// Scopes cancellation to this attempt's original pending files.
    pub fn native_attempt_generation(&self) -> Option<&str> {
        Some(self.native.generation())
    }

    /// Returns only the existing nonsensitive UI status.
    pub fn state(&self) -> &AuthState {
        self.native.state()
    }

    /// Polls bounded presentation messages without blocking the frame.
    pub fn poll(&mut self) {
        self.native.poll();
    }

    /// Cancels service work without discarding pending credential cleanup.
    pub fn request_cancel(&mut self) {
        self.native.cancel(false);
    }

    /// Dismisses an explicit prompt after cancelling the original attempt.
    pub fn cancel_prompt(&mut self) {
        self.native.cancel(true);
    }

    /// Native cleanup has one original completion receipt.
    pub fn cleanup_handed_off(&self) -> bool {
        self.native.complete()
    }

    /// Reports completion only after bounded persistence and worker cleanup finish.
    pub fn cleanup_complete(&self) -> bool {
        self.native.complete()
    }
}

#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
impl Drop for AuthSupervisor {
    fn drop(&mut self) {
        self.request_cancel();
    }
}

#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
fn valid_https_uri(value: &str) -> bool {
    Url::parse(value).is_ok_and(|uri| {
        uri.scheme() == "https"
            && uri.host_str().is_some()
            && uri.username().is_empty()
            && uri.password().is_none()
    })
}

#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
fn valid_code(value: &str) -> bool {
    !value.is_empty() && value.len() <= 64 && value.bytes().all(|byte| byte.is_ascii_graphic())
}

#[cfg(all(
    any(test, feature = "test-support"),
    feature = "native-networking",
    not(target_arch = "wasm32")
))]
pub mod test_support;
