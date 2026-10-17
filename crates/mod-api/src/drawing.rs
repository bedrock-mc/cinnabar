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
/// Maximum physical-pixel expansion of a crisp host outline.
pub const MAX_MASK_OUTLINE_RADIUS: f32 = 4.0;
/// Downsample levels shared by every effect reading a selection's halo.
pub const MASK_BLUR_LEVELS: usize = 4;
/// Maximum physical-pixel radius of a host-composed selection halo.
pub const MAX_MASK_GLOW_RADIUS: f32 = (1 << MASK_BLUR_LEVELS) as f32;

/// Bounded host composition; colour components use the display's sRGB encoding.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MaskStyle {
    pub outline: [f32; 4],
    pub outline_radius: f32,
    pub glow: [f32; 4],
    pub glow_radius: f32,
}

impl MaskStyle {
    /// Rejects non-finite colours, coverage or radii before retaining presentation state.
    pub fn valid(self) -> bool {
        self.outline
            .iter()
            .chain(&self.glow)
            .all(|value| value.is_finite() && (0.0..=1.0).contains(value))
            && (0.0..=MAX_MASK_OUTLINE_RADIUS).contains(&self.outline_radius)
            && (0.0..=MAX_MASK_GLOW_RADIUS).contains(&self.glow_radius)
    }
}

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
