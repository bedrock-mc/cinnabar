//! Connection fences and admission for personal-mod drawing and protected observations.

use crate::{ModGrants, ModHost};
use mod_api::drawing::{DrawingPermissions, DrawingSession};

/// Tracks whether a revoked guest still needs an admission attempt under its latest fence.
pub(crate) enum DrawingRestart {
    None,
    Pending,
    Attempted,
}

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
        // Each legacy capability retains its own consent in local and remote worlds.
        let protected = policy.entity_observe && policy.render_through_walls;
        grants.entities &= protected;
        grants.terrain &= policy.render_through_walls;
        grants.block_highlights &= policy.render_through_walls;
        grants
    }
}

impl ModHost {
    /// Binds authority before callbacks. A changed fence stops the guest because previously
    /// disclosed observations cannot be removed from its memory. Reload creates a fresh guest.
    pub fn bind_drawing_session(&mut self, session: DrawingSession) -> bool {
        if self.drawing_session == Some(session) {
            return false;
        }
        if self.is_active() || self.needs_drawing_restart() {
            self.drawing_restart = DrawingRestart::Pending;
        }
        self.pending_events.clear();
        self.instance.revoke();
        self.drawing_session = Some(session);
        self.instance
            .bind_drawing_policy(self.grants.for_drawing_session(session), session);
        true
    }

    /// Returns the fence used to admit callbacks and prepare an off-frame replacement.
    pub fn drawing_session(&self) -> Option<DrawingSession> {
        self.drawing_session
    }

    /// Whether a session fence needs a fresh guest, independently of registration changes.
    pub fn needs_drawing_restart(&self) -> bool {
        !matches!(self.drawing_restart, DrawingRestart::None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_worlds_preserve_each_individual_user_permission() {
        let request = ModGrants {
            commands: vec!["fixture".into()],
            ..Default::default()
        };
        for capability in ModGrants::CAPABILITIES {
            for granted in [false, true] {
                let mut grants = ModGrants::default();
                (capability.set)(&mut grants, granted, Some(&request));
                for local in [false, true] {
                    assert_eq!(
                        grants.for_drawing_session(DrawingSession {
                            connection: 1,
                            local,
                            permissions: DrawingPermissions::ALL,
                            ..Default::default()
                        }),
                        grants,
                        "{}: local={local}, granted={granted}",
                        capability.name,
                    );
                }
            }
        }
    }

    #[test]
    fn restricted_session_preserves_player_consent_but_denies_protected_drawing() {
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
        assert!(effective.render && effective.hud && effective.players);
        assert!(!effective.entities && !effective.terrain);
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
    #[test]
    fn drawing_capability_names_agree_across_admission_and_negotiation() {
        use experience_sdk::mod_manifest::ModManifest;
        use server_experience::manifest::Permission;
        for &name in DrawingPermissions::NAMES {
            let grants: ModGrants =
                serde_json::from_value(serde_json::json!({(name): true})).unwrap();
            let descriptor = ModGrants::CAPABILITIES
                .iter()
                .find(|capability| capability.name == name)
                .unwrap();
            assert!((descriptor.enabled)(&grants));
            let permission: Permission = serde_json::from_value(name.into()).unwrap();
            assert_eq!(serde_json::to_value(permission).unwrap(), name);
            let manifest: ModManifest = serde_json::from_value(serde_json::json!({"id":"drawing", "version":"1", "api":experience_sdk::mod_manifest::API, "permissions":[name]})).unwrap();
            let request = ModGrants::from_manifest(&manifest).drawing_permissions();
            for &other in DrawingPermissions::NAMES {
                assert_eq!(grants.drawing_permissions().enabled(other), other == name);
                assert_eq!(request.enabled(other), other == name);
            }
        }
    }
}
