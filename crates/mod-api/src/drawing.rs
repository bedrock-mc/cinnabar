//! Authority and lifetimes for the versioned world drawing interface.

macro_rules! permissions {
    ($($field:ident),+ $(,)?) => {
        /// Independently consented drawing authority. Absence always denies a capability.
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
        pub struct DrawingPermissions { $(pub $field: bool,)+ }
        impl DrawingPermissions {
            /// Canonical capability names, generated from their owning fields.
            pub const NAMES: &'static [&'static str] = &[$(stringify!($field),)+];
            /// Host ceiling for an explicitly local world; user consent still applies.
            pub const ALL: Self = Self { $($field: true,)+ };
            /// Retains only authority independently granted by both inputs.
            pub const fn intersect(self, other: Self) -> Self { Self { $($field: self.$field && other.$field,)+ } }
            /// Reads one canonical permission name; unknown names never grant authority.
            pub fn enabled(self, name: &str) -> bool { match name { $(stringify!($field) => self.$field,)+ _ => false } }
            /// Intersects host limits, package request, user consent and current session policy.
            pub const fn effective(host: Self, requested: Self, consent: Self, session: Self) -> Self {
                host.intersect(requested).intersect(consent).intersect(session)
            }
        }
    };
}
permissions! { world_draw, entity_observe, entity_highlight, render_through_walls, render_material, render_post }

/// A token belongs to one owner incarnation; slots never authorize another incarnation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DrawingHandle {
    pub owner: u64,
    pub generation: u64,
    pub slot: u64,
}

/// Trusted connection and dimension fence, independent of a server address.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DrawingSession {
    pub connection: u64,
    pub dimension: i32,
    /// Set only by the trusted local-world launcher.
    pub local: bool,
    pub permissions: DrawingPermissions,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_authority_input_can_deny() {
        let all = DrawingPermissions::ALL;
        let denied = DrawingPermissions::default();
        for missing in 0..4 {
            let mut inputs = [all; 4];
            inputs[missing] = denied;
            assert_eq!(
                DrawingPermissions::effective(inputs[0], inputs[1], inputs[2], inputs[3]),
                denied
            );
        }
        assert_eq!(DrawingPermissions::effective(all, all, all, all), all);
    }
}
