//! Packet-scene adapter for the same actor masks used by component extensions.
use mod_api::{
    drawing::{DrawingHandle, DrawingSession, MaskDepth, MaskEntity},
    native_render::Scene,
};
use mod_render::masks::MaskStore;
use std::sync::Arc;

pub(super) struct Highlights {
    store: MaskStore,
    scope: DrawingSession,
    targets: Vec<(MaskEntity, u64)>,
    handles: Vec<DrawingHandle>,
    pub(super) dirty: bool,
}
impl Highlights {
    /// Shares the drawing owner's namespace without exposing handles to the packet guest.
    pub(super) fn new(owner: u64) -> Self {
        Self {
            store: MaskStore::new(owner),
            scope: Default::default(),
            targets: Vec::new(),
            handles: Vec::new(),
            dirty: false,
        }
    }
    /// Actor incarnations and admitted vertex costs come only from the trusted render frame.
    pub(super) fn targets(&mut self, targets: Vec<(MaskEntity, u64)>) {
        if self.targets != targets {
            self.targets = targets;
            self.dirty = true;
        }
    }
    /// Reuses selections while replacing despawned or reused runtime IDs at their owner.
    pub(super) fn update(
        &mut self,
        scene: Option<&Scene>,
        scope: DrawingSession,
    ) -> Arc<[mod_render::masks::Mask]> {
        self.dirty = false;
        if self.scope != scope {
            self.scope = scope;
            self.store.bind(scope);
            self.handles.clear();
        }
        self.store.publish_targets(&self.targets);
        self.store.begin();
        let groups = scene
            .map(|scene| scene.entities.as_slice())
            .unwrap_or_default();
        for handle in self.handles.drain(groups.len().min(self.handles.len())..) {
            let _ = self.store.remove(handle);
        }
        for (index, group) in groups.iter().enumerate() {
            let handle = if let Some(&handle) = self.handles.get(index) {
                handle
            } else {
                let Ok(handle) = self.store.create(MaskDepth::Always) else {
                    break;
                };
                self.handles.push(handle);
                handle
            };
            let entities: Vec<_> = group
                .runtime_ids
                .iter()
                .filter_map(|id| self.store.entity(*id).ok())
                .collect();
            if self.store.select(handle, &entities).is_err() {
                // An oversized or retired actor cannot retain yesterday's selected mesh.
                let _ = self.store.select(handle, &[]);
            }
            let _ = self.store.set_style(handle, Some(group.style));
        }
        self.store.commit()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mod_api::drawing::DrawingPermissions;
    /// Drives the adapter through a live selection, reused actor ID, despawn and revocation.
    #[test]
    fn actor_incarnations_and_scope_release_follow_the_shared_mask_owner() {
        let mut highlights = Highlights::new(9);
        let scope = DrawingSession {
            connection: 1,
            dimension: 0,
            permissions: DrawingPermissions::ALL,
            ..Default::default()
        };
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../crates/mod-api/native-render-v1.json"
        ))
        .unwrap();
        let scene: Scene = serde_json::from_value(fixture["scene"].clone()).unwrap();
        let actor = MaskEntity {
            connection: 1,
            dimension: 0,
            runtime_id: 42,
            spawn_revision: 1,
        };
        highlights.targets(vec![(actor, 36)]);
        let before = highlights.update(Some(&scene), scope);
        assert_eq!(before[0].entities, [actor]);
        assert_eq!(before[0].style, Some(scene.entities[0].style));
        assert!(Arc::ptr_eq(
            &before,
            &highlights.update(Some(&scene), scope)
        ));
        let replacement = MaskEntity {
            spawn_revision: 2,
            ..actor
        };
        highlights.targets(vec![(replacement, 36)]);
        assert_eq!(
            highlights.update(Some(&scene), scope)[0].entities,
            [replacement]
        );
        highlights.targets(Vec::new());
        assert!(
            highlights.update(Some(&scene), scope)[0]
                .entities
                .is_empty()
        );
        assert!(highlights.update(None, scope).is_empty());
        highlights.targets(vec![(replacement, 36)]);
        assert!(
            highlights
                .update(Some(&scene), DrawingSession::default())
                .is_empty()
        );
    }
}
