//! Pending menu actions owned by the existing session dispatcher.

use super::*;

/// Session requests raised by menu actions, for the session controller to take.
#[derive(Debug, Default)]
pub(super) struct SessionIntents {
    pub(super) join: Option<JoinIntent>,
    #[cfg(feature = "native-networking")]
    pub(super) gathering: Option<launcher_host::native_account::NativeGatheringSelection>,
    #[cfg(feature = "native-networking")]
    pub(super) native_world: Option<native_local_world::NativeLocalWorldIntent>,
    /// The queued join waits for the saved account's check until this passes.
    pub(super) join_sign_in_deadline: Option<std::time::Instant>,
    /// The background sign-in check serves a join: it may finish, but never prompts.
    pub(super) join_sign_in_check: bool,
    /// A selected native account may not silently become a signed-out join.
    pub(super) native_auth_required: bool,
    pub(super) disconnect: bool,
    pub(super) respawn: bool,
    pub(super) exit: bool,
}
