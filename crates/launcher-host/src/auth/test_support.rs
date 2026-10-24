//! Deterministic native presentation fixtures without processes or service requests.

use super::*;

/// Supplies native presenter state without a helper process, HTTP, credentials or wall-clock waiting.
#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
pub fn native_state(state: AuthState) -> AuthSupervisor {
    AuthSupervisor::from_native(native::NativeAuth::from_state(state))
}

/// Observes the original native attempt's cancellation without changing completion.
pub fn cancellation_requested(supervisor: &AuthSupervisor) -> bool {
    supervisor.native.cancellation_requested()
}
