//! Camera draw lists of the geometry resource: which resident sections each camera view
//! draws, rebuilt only when what they depend on moves.

use std::hash::{BuildHasher, Hash, Hasher};

use bevy::platform::hash::FixedHasher;
use bevy::render::renderer::{RenderDevice, RenderQueue};
use render_scene::ChunkRenderInstance;

use super::super::draw::{DrawInputs, ViewDraws, eye_section_centre};
use super::super::residency::ResidentFaces;
use super::{GeometryViews, ShaderpackGeometry};
use crate::chunk::shaderpack_sources::PackedChunkSources;

/// Draw lists per camera view of its visible resident sections within `far` horizontally of
/// the camera's section, as Java culls render sections by render distance from it, leaving
/// out faces the camera sees only from behind. A view's list is rebuilt only when the
/// sections it lists, the camera's section or the facing groups it sees change, and the
/// sections are not even walked while the residency, the hidden set, the visible set and the
/// camera's grid cells all hold.
pub(super) fn build_draws(
    geometry: &mut ShaderpackGeometry,
    device: &RenderDevice,
    queue: &RenderQueue,
    sources: &PackedChunkSources,
    views: &GeometryViews,
    far: Option<f32>,
) {
    let epoch = geometry.residency.epoch;
    let mut max_faces = 0;
    geometry
        .views
        .retain(|entity, _| views.get(*entity).is_ok());
    let reach = far.map(|far| f64::from(far) + 8.0);
    let revisions = (
        geometry.residency.revision(),
        epoch,
        geometry.residency.generation,
        sources.hidden_revision(),
        reach.map(f64::to_bits),
    );
    for (entity, view, visible, _) in views.iter() {
        let eye = view.world_from_view.translation().as_dvec3();
        let visible = || {
            visible
                .get::<ChunkRenderInstance>()
                .into_iter()
                .flat_map(|class| class.iter_visible())
                .map(|(entity, _)| *entity)
        };
        let source = camera_source(revisions, eye, visible());
        if geometry
            .views
            .get(&entity)
            .is_some_and(|draws| draws.is_from(source))
        {
            continue;
        }
        let centre = eye_section_centre(eye);
        let within = |faces: &ResidentFaces| {
            reach.is_none_or(|reach| {
                let [x, _, z] = faces.origin;
                let dx = f64::from(x) + 8.0 - centre.x;
                let dz = f64::from(z) + 8.0 - centre.z;
                dx * dx + dz * dz <= reach * reach
            })
        };
        let inputs = DrawInputs::new(
            visible()
                .filter(|entity| !sources.is_hidden(*entity))
                .filter_map(|entity| geometry.residency.sections.get(&entity))
                .filter(|faces| within(faces)),
            Some(eye),
            epoch,
            geometry.residency.generation,
            Some(eye),
        );
        let key = inputs.key();
        let draws = geometry.views.entry(entity).or_default();
        draws.set_source(source);
        if draws.is_current(key) {
            continue;
        }
        let built = ViewDraws::build(&inputs);
        max_faces = max_faces.max(built.max_faces());
        draws.replace(built, key, device, queue, geometry.multi_draw);
    }
    if max_faces > 0 {
        geometry.ensure_index_buffer(device, max_faces);
    }
}

/// Keeps stationary-view reuse while invalidating native face sorting for camera motion.
fn camera_source(
    revisions: impl Hash,
    eye: bevy::math::DVec3,
    visible: impl Iterator<Item = bevy::prelude::Entity>,
) -> u64 {
    let mut hasher = FixedHasher.build_hasher();
    (revisions, eye.to_array().map(f64::to_bits)).hash(&mut hasher);
    for entity in visible {
        entity.hash(&mut hasher);
    }
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_motion_inside_a_section_invalidates_cached_face_order() {
        use bevy::math::DVec3;
        let eye = DVec3::new(1.0, 1.0, 1.0);
        let source = camera_source(0, eye, std::iter::empty());
        let mut draws = ViewDraws::default();
        draws.set_source(source);
        assert!(draws.is_from(camera_source(0, eye, std::iter::empty())));
        assert!(!draws.is_from(camera_source(
            0,
            DVec3::new(15.0, 1.0, 1.0),
            std::iter::empty()
        )));
    }
}
