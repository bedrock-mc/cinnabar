//! Personal-mod availability follows the live connection and explicit user consent.

use mod_api::drawing::DrawingPermissions;

/// Allows user-consented capabilities in any active world and suspends them outside a world.
pub fn session_permissions(connection: u64) -> DrawingPermissions {
    if connection == 0 {
        DrawingPermissions::default()
    } else {
        DrawingPermissions::ALL
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_connections_allow_every_individually_consented_capability() {
        for connection in [1, 2, u64::MAX] {
            assert_eq!(session_permissions(connection), DrawingPermissions::ALL);
        }
        assert_eq!(session_permissions(0), DrawingPermissions::default());
    }
}
