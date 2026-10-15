use bevy::{
    prelude::{Deref, DerefMut, GlobalTransform, Projection, Resource},
    render::extract_resource::ExtractResource,
};
use particles::{
    AtlasPatch, DrawLists, ParticleInstance, ParticleSystem, ParticleView, ParticleWorld,
};
use std::sync::Arc;
/// The particle simulation as a Bevy resource.
#[derive(Resource, Default, Deref, DerefMut)]
pub struct ParticleSimulation(pub ParticleSystem);

/// The frame's particle draw data, extracted to the render world each frame.
#[derive(Resource, ExtractResource, Clone, Default)]
#[extract_app(bevy::render::RenderApp)]
pub struct ParticleGpuFrame {
    pub base: Option<Arc<[u8]>>,
    /// Physical dimension of the current atlas, including its dynamic tile rows.
    pub atlas_side: u32,
    pub patch_seq: u64,
    pub patches: Arc<[AtlasPatch]>,
    pub opaque: Arc<[ParticleInstance]>,
    pub blend: Arc<[ParticleInstance]>,
    pub add: Arc<[ParticleInstance]>,
    /// Drawable particles hidden by the current cave graph this frame.
    pub cave_hidden: usize,
}

impl ParticleGpuFrame {
    #[must_use]
    pub fn instance_count(&self) -> usize {
        self.opaque.len() + self.blend.len() + self.add.len()
    }

    /// Publishes the current camera-sorted instance lists for extraction, keeping each
    /// published list whose bytes are unchanged; returns whether anything changed.
    fn set_lists(&mut self, lists: DrawLists) -> bool {
        let mut changed = self.cave_hidden != lists.cave_hidden;
        self.cave_hidden = lists.cave_hidden;
        changed |= republish(&mut self.blend, lists.blend);
        changed |= republish(&mut self.add, lists.add);
        changed |= republish(&mut self.opaque, lists.opaque);
        changed
    }
}

/// Replaces `published` with `list` unless their bytes are identical, so an unchanged list keeps
/// its allocation; bytewise equality also distinguishes `-0.0` and NaN payloads from `0.0`.
fn republish(published: &mut Arc<[ParticleInstance]>, list: Vec<ParticleInstance>) -> bool {
    if bytemuck::cast_slice::<_, u8>(published) == bytemuck::cast_slice::<_, u8>(&list) {
        return false;
    }
    *published = list.into();
    true
}

/// Camera frustum basis from a Bevy camera transform and projection.
#[must_use]
pub fn particle_view(transform: &GlobalTransform, projection: &Projection) -> ParticleView {
    let (right, up, forward) = (transform.right(), transform.up(), transform.forward());
    let half_diagonal = match projection {
        Projection::Perspective(perspective) => {
            let half_height = (perspective.fov * 0.5).tan();
            let half_width = half_height * perspective.aspect_ratio;
            half_width.hypot(half_height).atan()
        }
        _ => std::f32::consts::FRAC_PI_2,
    };
    ParticleView {
        position: transform.translation().to_array(),
        right: right.to_array(),
        up: up.to_array(),
        forward: forward.to_array(),
        half_diagonal,
    }
}

/// Ticks the simulation and refreshes draw data, using the caller's conservative
/// world-box visibility test only for instance building; call once per frame. Returns whether
/// `frame` changed, so callers can mark it for extraction only then.
#[must_use]
pub fn update_particle_frame(
    system: &mut ParticleSystem,
    frame: &mut ParticleGpuFrame,
    dt: f32,
    view: &ParticleView,
    world: &dyn ParticleWorld,
    hides_box: impl FnMut([f32; 3], [f32; 3]) -> bool,
) -> bool {
    system.set_camera(view.position);
    system.tick(dt, world);
    let lists = {
        let span = bevy::log::info_span!(
            "particle.instance_build",
            cave_hidden = bevy::log::tracing::field::Empty,
            submitted = bevy::log::tracing::field::Empty,
            light_lookups = bevy::log::tracing::field::Empty,
            light_cache_hits = bevy::log::tracing::field::Empty,
        );
        let _zone = span.enter();
        let lists = system.build_draw_with_visibility(view, world, hides_box);
        span.record("cave_hidden", lists.cave_hidden);
        span.record("light_lookups", lists.light_lookups);
        span.record("light_cache_hits", lists.light_cache_hits);
        span.record(
            "submitted",
            lists.opaque.len() + lists.blend.len() + lists.add.len(),
        );
        lists
    };
    let mut changed = frame.set_lists(lists);
    let base = system.atlas_base();
    if frame
        .base
        .as_ref()
        .is_none_or(|old| !Arc::ptr_eq(old, &base))
    {
        frame.base = Some(base);
        frame.atlas_side = system.atlas().side();
        frame.patch_seq = 0;
        frame.patches = Arc::default();
        changed = true;
    }
    let seq = system.atlas().patch_seq();
    if seq != frame.patch_seq {
        frame.patches = system.atlas().patches().into();
        frame.patch_seq = seq;
        changed = true;
    }
    changed
}

#[cfg(test)]
mod tests;
