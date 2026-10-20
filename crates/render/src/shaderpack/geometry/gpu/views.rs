//! Builds terrain draw lists for the retained camera views.

use super::{GeometryViews, ShaderpackGeometry};
use crate::chunk::shaderpack_sources::PackedChunkSources;
use crate::shaderpack::geometry::draw::{DrawInputs, ViewDraws, eye_section_centre};
use crate::shaderpack::geometry::residency::ResidentFaces;
use bevy::render::renderer::{RenderDevice, RenderQueue};
use render_scene::ChunkRenderInstance;

/// Draw lists per camera view of its visible resident sections within `far` horizontally of
/// the camera's section, as Java culls render sections by render distance from it, leaving
/// out faces the camera sees only from behind. A view's list is rebuilt only when the
/// sections it lists, the camera's section or the facing groups it sees change.
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
    for (entity, view, visible, _) in views.iter() {
        let eye = view.world_from_view.translation().as_dvec3();
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
            visible
                .get::<ChunkRenderInstance>()
                .into_iter()
                .flat_map(|class| class.iter_visible())
                .map(|(entity, _)| *entity)
                .filter(|entity| !sources.is_hidden(*entity))
                .filter_map(|entity| geometry.residency.sections.get(&entity))
                .filter(|faces| within(faces)),
            Some(eye),
            epoch,
            geometry.residency.generation,
            Some(eye),
        );
        let key = inputs.key();
        if geometry
            .views
            .get(&entity)
            .is_some_and(|draws| draws.is_current(key))
        {
            continue;
        }
        let built = ViewDraws::build(&inputs);
        max_faces = max_faces.max(built.max_faces());
        geometry.views.entry(entity).or_default().replace(
            built,
            key,
            device,
            queue,
            geometry.multi_draw,
        );
    }
    if max_faces > 0 {
        geometry.ensure_index_buffer(device, max_faces);
    }
}
