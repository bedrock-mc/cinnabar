//! Authority and lifetimes for the versioned world drawing interface.

/// Generated resource operations and records from the world drawing WIT contract.
pub use crate::bindings::cinnabar::world_draw::drawing as api;

pub mod shapes;

/// Places retained geometry with depth testing, white tint and an identity transform.
/// Set width and width mode explicitly when a polyline needs a different stroke.
pub fn instance(geometry: api::Handle, anchor: api::Anchor) -> api::Instance {
    api::Instance {
        geometry,
        anchor,
        transform: api::Transform {
            translation: (0., 0., 0.),
            rotation: (0., 0., 0., 1.),
            scale: (1., 1., 1.),
        },
        tint: (1., 1., 1., 1.),
        depth: api::Depth::Test,
        blend: api::Blend::Premultiplied,
        width: 1.,
        width_mode: api::WidthMode::World,
        join: api::Join::Bevel,
        cap: api::Cap::Butt,
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sdk_instance_uses_the_generated_contract_and_safe_defaults() {
        let handle = api::Handle {
            owner: 1,
            generation: 2,
            slot: 3,
        };
        let spec = instance(handle, api::Anchor::Camera);
        assert_eq!(
            (
                spec.geometry.owner,
                spec.geometry.generation,
                spec.geometry.slot
            ),
            (1, 2, 3)
        );
        assert!(matches!(spec.anchor, api::Anchor::Camera));
        assert_eq!(spec.transform.translation, (0., 0., 0.));
        assert_eq!(spec.transform.rotation, (0., 0., 0., 1.));
        assert_eq!(spec.transform.scale, (1., 1., 1.));
        assert_eq!(spec.tint, (1., 1., 1., 1.));
        assert_eq!(spec.depth, api::Depth::Test);
        assert_eq!(spec.blend, api::Blend::Premultiplied);
        assert_eq!(spec.width, 1.);
        assert_eq!(spec.width_mode, api::WidthMode::World);
        assert_eq!(spec.join, api::Join::Bevel);
        assert_eq!(spec.cap, api::Cap::Butt);
    }

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
