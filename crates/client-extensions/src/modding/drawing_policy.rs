//! Adapts authenticated bundle scope without granting other personal components authority.

use mod_api::drawing::DrawingPermissions;
use server_experience::{manifest::Permission, negotiation::Grant};
use std::sync::Arc;

/// An immutable drawing scope captured from a live authenticated grant for worker admission.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DrawingGrant(Option<Arc<server_experience::manifest::Scope>>);

impl DrawingGrant {
    /// Captures only unexpired authenticated scope; callers recheck it before publication.
    pub fn capture(grant: Option<&Grant>, now_unix: u64) -> Self {
        Self(
            grant
                .filter(|grant| now_unix < grant.expires_unix)
                .map(|grant| Arc::new(grant.offer.offer.scope.clone())),
        )
    }

    /// Evaluates each candidate digest independently within the captured authority.
    pub fn permissions(&self, digest: &[u8; 32]) -> DrawingPermissions {
        scope_permissions(self.0.as_deref(), digest)
    }
}

/// A remote session always permits bounded plain geometry. Elevated permissions apply only
/// to the explicitly named personal-mod digest and expire with the authenticated negotiation grant.
pub fn remote_permissions(
    grant: Option<&Grant>,
    digest: &[u8; 32],
    now_unix: u64,
) -> DrawingPermissions {
    scope_permissions(
        grant
            .filter(|grant| now_unix < grant.expires_unix)
            .map(|grant| &grant.offer.offer.scope),
        digest,
    )
}

/// Applies one authenticated scope without transferring authority between component digests.
fn scope_permissions(
    scope: Option<&server_experience::manifest::Scope>,
    digest: &[u8; 32],
) -> DrawingPermissions {
    let plain = DrawingPermissions {
        world_draw: true,
        ..Default::default()
    };
    let Some(scope) = scope else {
        return plain;
    };
    let digest = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if !scope.personal_mods.contains(&digest) {
        return plain;
    }
    let has = |permission| scope.permissions.contains(&permission);
    DrawingPermissions {
        world_draw: has(Permission::WorldDraw),
        entity_observe: has(Permission::EntityObserve),
        entity_highlight: has(Permission::EntityHighlight),
        render_through_walls: has(Permission::RenderThroughWalls),
        render_material: has(Permission::RenderMaterial),
        render_post: has(Permission::RenderPost),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn absent_negotiation_cannot_elevate_a_personal_mod() {
        let permissions = remote_permissions(None, &[0; 32], 0);
        assert!(permissions.world_draw);
        assert!(
            !permissions.entity_observe
                && !permissions.render_through_walls
                && !permissions.render_post
        );
    }
}
