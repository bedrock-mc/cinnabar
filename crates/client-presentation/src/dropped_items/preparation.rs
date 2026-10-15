//! Conservative rejection after model admission and before per-copy publication and lighting.

use super::{ModelCache, ModelKey};
use client_world::{DroppedItemView, MAX_DROPPED_ITEM_COPIES};
use render_model::{
    ActorCullView, DroppedItemInstance, DroppedItemShape, DroppedItemSpawnPose,
    dropped_item::DroppedItemFrustum, native_dropped_item_transform,
};

/// A shared draw list may reject against one view only when it has exactly one 3D camera.
pub(super) fn publication_frustum(
    view: Option<ActorCullView>,
    single_view: bool,
) -> Option<DroppedItemFrustum> {
    let view = view
        .filter(|view| single_view && view.max_distance.is_finite() && view.max_distance > 0.0)?;
    DroppedItemFrustum::new(view.clip_from_world, view.camera_position)
}

/// Removes first-render captures only when their actor lifetime is no longer published.
pub(super) fn retain_lifetimes(cache: &mut ModelCache, dropped: &[DroppedItemView]) {
    let live = dropped
        .iter()
        .map(|view| (view.runtime_id, view.spawn_revision))
        .collect::<std::collections::HashSet<_>>();
    cache
        .spawn_poses
        .retain(|lifetime, _| live.contains(lifetime));
}

/// One item's current interpolation and its lifetime's original first-render inputs.
pub(super) struct DropPreparation<'a> {
    pub view: &'a DroppedItemView,
    pub native_origin: [f32; 3],
    pub camera: Option<([f32; 3], f32)>,
    pub frustum: Option<&'a DroppedItemFrustum>,
}

/// Successful admission, including items omitted only from this frame's draw list.
pub(super) struct PreparedDrop {
    model: u32,
    position: [f32; 3],
    transforms: [[[f32; 4]; 3]; MAX_DROPPED_ITEM_COPIES],
    visible: bool,
}

impl DropPreparation<'_> {
    /// Captures the spawn pose only after successful admission. Cold models still resolve in
    /// order, preserving atlas pressure and transient failures even while outside the view.
    pub(super) fn model(
        &self,
        cache: &mut ModelCache,
        key: &ModelKey,
        shape: DroppedItemShape,
        resolve: impl FnOnce(&mut ModelCache) -> Option<u32>,
    ) -> Option<PreparedDrop> {
        let model = cache.resident(key).or_else(|| resolve(cache))?;
        let pose = *cache
            .spawn_poses
            .entry((self.view.runtime_id, self.view.spawn_revision))
            .or_insert_with(|| DroppedItemSpawnPose::new(self.native_origin, self.camera));
        let mut position = self.view.position;
        position[1] +=
            pose.bob(self.view.age_ticks, self.view.bob_phase, shape) * self.view.bob_multiplier;
        let yaw = pose.yaw(self.view.yaw_radians);
        let mut transforms = [[[0.0; 4]; 3]; MAX_DROPPED_ITEM_COPIES];
        let count = usize::from(self.view.copy_count).min(MAX_DROPPED_ITEM_COPIES);
        for (rows, offset) in transforms
            .iter_mut()
            .zip(&self.view.copy_offsets)
            .take(count)
        {
            *rows = native_dropped_item_transform(
                position,
                yaw,
                *offset,
                self.view.render_scale,
                shape,
            );
        }
        let hidden = self.valid_inputs()
            && self
                .frustum
                .and_then(|frustum| cache.bounds(model).map(|bounds| (frustum, bounds)))
                .is_some_and(|(frustum, bounds)| {
                    transforms[..count].iter().all(|rows| {
                        bounds
                            .transformed(*rows)
                            .is_some_and(|bounds| frustum.rejects(bounds))
                    })
                });
        Some(PreparedDrop {
            model,
            position,
            transforms,
            visible: !hidden,
        })
    }

    /// Invalid animation data or copy counts cannot prove that a draw is outside the view.
    fn valid_inputs(&self) -> bool {
        let view = self.view;
        (1..=MAX_DROPPED_ITEM_COPIES).contains(&usize::from(view.copy_count))
            && view
                .position
                .into_iter()
                .chain(self.native_origin)
                .all(f32::is_finite)
            && [
                view.age_ticks,
                view.bob_phase,
                view.bob_multiplier,
                view.render_scale,
                view.yaw_radians,
            ]
            .into_iter()
            .all(f32::is_finite)
            && self
                .camera
                .is_none_or(|(eye, yaw)| eye.into_iter().chain([yaw]).all(f32::is_finite))
            && view.copy_offsets[..usize::from(view.copy_count)]
                .iter()
                .flatten()
                .all(|v| v.is_finite())
    }
}

impl PreparedDrop {
    /// Samples light only for surviving items, retaining copy order and item glint flags.
    pub(super) fn append(
        self,
        view: &DroppedItemView,
        instances: &mut Vec<DroppedItemInstance>,
        light: impl FnOnce([f32; 3]) -> (u8, u8),
    ) {
        let Self {
            model,
            position,
            transforms,
            visible,
        } = self;
        if !visible {
            return;
        }
        let (block_level, sky_level) = light(position);
        for world_from_item in transforms.into_iter().take(usize::from(view.copy_count)) {
            instances.push(DroppedItemInstance {
                model,
                world_from_item,
                block_level: u32::from(block_level),
                sky_level: u32::from(sky_level),
                overlay_rgba8: 0,
                glint: view.item.enchanted || view.item.glint.enabled().unwrap_or(false),
            });
        }
    }
}
