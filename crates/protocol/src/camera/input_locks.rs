//! Server input permissions, independent of camera selection and actor immobility.

/// Declares lock categories and their exhaustive iteration order together.
macro_rules! input_lock_categories {
    ($($name:ident = $value:expr),+ $(,)?) => {
        /// One input category named by the server input-lock packet.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        #[repr(u32)]
        pub enum ClientInputLock { $($name = $value),+ }

        impl ClientInputLock {
            /// Every declared category, including categories without a local request producer.
            pub const ALL: &'static [Self] = &[$(Self::$name),+];
        }
    };
}

input_lock_categories! {
    Camera = 1 << 1,
    Movement = 1 << 2,
    LateralMovement = 1 << 4,
    Sneak = 1 << 5,
    Jump = 1 << 6,
    Mount = 1 << 7,
    Dismount = 1 << 8,
    MoveForward = 1 << 9,
    MoveBackward = 1 << 10,
    MoveLeft = 1 << 11,
    MoveRight = 1 << 12,
}

/// Retained server locks; a new packet replaces the previous categories.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ClientInputLocks(u32);

impl ClientInputLocks {
    /// Accepts unknown bits while excluding the actor-owned immobility category.
    #[must_use]
    pub const fn from_bits(bits: u32) -> Self {
        Self(bits & !(1 << 3))
    }

    /// Resolves broad movement categories before checking a specific input.
    #[must_use]
    pub const fn blocks(self, category: ClientInputLock) -> bool {
        let movement = self.0 & ClientInputLock::Movement as u32 != 0;
        let lateral = movement || self.0 & ClientInputLock::LateralMovement as u32 != 0;
        self.0 & category as u32 != 0
            || match category {
                ClientInputLock::Camera | ClientInputLock::Movement => false,
                ClientInputLock::LateralMovement
                | ClientInputLock::MoveForward
                | ClientInputLock::MoveBackward
                | ClientInputLock::MoveLeft
                | ClientInputLock::MoveRight => lateral,
                ClientInputLock::Sneak
                | ClientInputLock::Jump
                | ClientInputLock::Mount
                | ClientInputLock::Dismount => movement,
            }
    }

    /// Filters simulation axes without altering the raw device observation.
    #[must_use]
    pub fn movement(self, [sideways, forward]: [f32; 2]) -> [f32; 2] {
        [
            if self.blocks(if sideways < 0.0 {
                ClientInputLock::MoveLeft
            } else {
                ClientInputLock::MoveRight
            }) {
                0.0
            } else {
                sideways
            },
            if self.blocks(if forward < 0.0 {
                ClientInputLock::MoveBackward
            } else {
                ClientInputLock::MoveForward
            }) {
                0.0
            } else {
                forward
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn movement_category_blocks_locomotion_without_locking_camera() {
        let locks = ClientInputLocks::from_bits(ClientInputLock::Movement as u32);
        assert_eq!(locks.movement([0.5, -1.0]), [0.0; 2]);
        assert!(locks.blocks(ClientInputLock::Jump));
        assert!(locks.blocks(ClientInputLock::Sneak));
        assert!(locks.blocks(ClientInputLock::Dismount));
        assert!(!locks.blocks(ClientInputLock::Camera));
        assert!(!ClientInputLocks::from_bits(1 << 3).blocks(ClientInputLock::Movement));
    }

    #[test]
    fn directional_locks_preserve_other_directions_and_unlock_replaces_them() {
        let locks = ClientInputLocks::from_bits(
            ClientInputLock::MoveForward as u32 | ClientInputLock::MoveLeft as u32,
        );
        assert_eq!(locks.movement([-0.5, 1.0]), [0.0; 2]);
        assert_eq!(locks.movement([0.5, -1.0]), [0.5, -1.0]);
        assert_eq!(
            ClientInputLocks::default().movement([-0.5, 1.0]),
            [-0.5, 1.0]
        );
    }
}
