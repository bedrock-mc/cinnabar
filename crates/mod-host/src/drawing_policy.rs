//! Connection fences and admission for personal-mod drawing and protected observations.

use crate::{ModGrants, ModHost};
use mod_api::drawing::{DrawingPermissions, DrawingSession};

impl ModGrants {
    /// Extracts the individually consented capabilities without extending legacy grants.
    pub fn drawing_permissions(&self) -> DrawingPermissions {
        DrawingPermissions {
            world_draw: self.world_draw,
            entity_observe: self.entity_observe,
            entity_highlight: self.entity_highlight,
            render_through_walls: self.render_through_walls,
            render_material: self.render_material,
            render_post: self.render_post,
        }
    }

    /// Narrows new capabilities to the component's explicit request.
    pub(crate) fn with_drawing_request(mut self, request: DrawingPermissions) -> Self {
        self.world_draw &= request.world_draw;
        self.entity_observe &= request.entity_observe;
        self.entity_highlight &= request.entity_highlight;
        self.render_through_walls &= request.render_through_walls;
        self.render_material &= request.render_material;
        self.render_post &= request.render_post;
        self
    }

    /// Applies the session ceiling to old and new producers before guest code can run.
    pub(crate) fn for_drawing_session(&self, session: DrawingSession) -> Self {
        let mut grants = self.clone();
        let policy = session.permissions;
        grants.world_draw &= policy.world_draw;
        grants.entity_observe &= policy.entity_observe;
        grants.entity_highlight &= policy.entity_highlight;
        grants.render_through_walls &= policy.render_through_walls;
        grants.render_material &= policy.render_material;
        grants.render_post &= policy.render_post;
        grants.render &= policy.world_draw;
        grants.render_depth &= policy.render_post;
        // Existing observations are not occlusion filtered. Deny them before disclosure.
        let protected = policy.entity_observe
            && policy.render_through_walls
            && (session.local || (self.entity_observe && self.render_through_walls));
        grants.players &= protected;
        grants.entities &= protected;
        grants.terrain &=
            policy.render_through_walls && (session.local || self.render_through_walls);
        grants.block_highlights &=
            policy.render_through_walls && (session.local || self.render_through_walls);
        grants
    }
}

impl ModHost {
    /// Binds authority before callbacks. A changed fence stops the guest because previously
    /// disclosed observations cannot be removed from its memory. Reload creates a fresh guest.
    pub fn bind_drawing_session(&mut self, session: DrawingSession) {
        if self.drawing_session == Some(session) {
            return;
        }
        self.drawing_restart = self.is_active() || self.is_revoked();
        self.revoke();
        self.drawing_session = Some(session);
        self.instance
            .bind_drawing_policy(self.grants.for_drawing_session(session), session);
    }

    /// Whether a session fence needs a fresh guest, independently of registration changes.
    pub fn needs_drawing_restart(&self) -> bool {
        self.drawing_restart
    }

    /// Digest of the admitted source, used only for an explicit signed bundle scope.
    pub fn source_digest(&self) -> [u8; 32] {
        self.admitted_digest
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_default_denies_protected_producers_but_keeps_plain_geometry() {
        let grants = ModGrants {
            render: true,
            players: true,
            entities: true,
            terrain: true,
            block_highlights: true,
            render_depth: true,
            hud: true,
            ..Default::default()
        };
        let effective = grants.for_drawing_session(DrawingSession {
            connection: 1,
            permissions: DrawingPermissions {
                world_draw: true,
                ..Default::default()
            },
            ..Default::default()
        });
        assert!(effective.render && effective.hud);
        assert!(!effective.players && !effective.entities && !effective.terrain);
        assert!(!effective.block_highlights && !effective.render_depth);
        assert_eq!(
            grants.for_drawing_session(DrawingSession {
                permissions: DrawingPermissions::ALL,
                local: true,
                ..Default::default()
            }),
            grants
        );
    }
}
