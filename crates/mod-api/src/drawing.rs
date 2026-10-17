//! Authority and lifetimes for the versioned world drawing interface.

/// Independently consented drawing authority. Absence always denies a capability.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DrawingPermissions {
    pub world_draw: bool,
    pub entity_observe: bool,
    pub entity_highlight: bool,
    pub render_through_walls: bool,
    pub render_material: bool,
    pub render_post: bool,
}

impl DrawingPermissions {
    /// Host ceiling for an explicitly local world; user consent still applies.
    pub const ALL: Self = Self {
        world_draw: true,
        entity_observe: true,
        entity_highlight: true,
        render_through_walls: true,
        render_material: true,
        render_post: true,
    };

    /// Retains only permissions independently authorized by both inputs.
    pub const fn intersect(self, other: Self) -> Self {
        Self {
            world_draw: self.world_draw && other.world_draw,
            entity_observe: self.entity_observe && other.entity_observe,
            entity_highlight: self.entity_highlight && other.entity_highlight,
            render_through_walls: self.render_through_walls && other.render_through_walls,
            render_material: self.render_material && other.render_material,
            render_post: self.render_post && other.render_post,
        }
    }

    /// Intersects host limits, the package request, user consent and current session policy.
    pub const fn effective(host: Self, requested: Self, consent: Self, session: Self) -> Self {
        host.intersect(requested)
            .intersect(consent)
            .intersect(session)
    }
}

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

/// Retained union selections per component, including empty selections.
pub const MAX_MASKS: usize = 2;
/// Distinct actors selected by one mask.
pub const MAX_MASK_ENTITIES: usize = 32;
/// Actor vertices admitted to one selection, including equipment.
pub const MAX_MASK_VERTICES: u64 = 262_144;
/// Total attachment bytes across masks and views, including MSAA resolve targets.
pub const MAX_MASK_ATTACHMENT_BYTES: u64 = 256 * 1024 * 1024;
/// Mask imports admitted in one callback.
pub const MAX_MASK_CALLS: u32 = 64;

/// An actor incarnation, shared with trusted frame publication; never disclosed to shaders.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MaskEntity {
    pub connection: u64,
    pub dimension: i32,
    pub runtime_id: u64,
    pub spawn_revision: u64,
}

/// Draw-time depth authority cannot be changed after mask creation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MaskDepth {
    Test,
    Always,
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
