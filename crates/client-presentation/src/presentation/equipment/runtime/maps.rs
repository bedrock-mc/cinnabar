//! Immutable paper and image planes shared by both held map slots.
use super::*;

impl EquipmentRuntime {
    /// Shares pack paper at its authored resolution, falling back to the startup pack.
    pub(crate) fn map_background(&self) -> Option<render_model::HandItemAtlas> {
        let identifier = assets::entity::MAP_BACKGROUND_TEXTURE_IDENTIFIER;
        let texture = self
            .pack
            .as_ref()
            .and_then(|pack| pack.catalog.as_ref())
            .into_iter()
            .chain(self.catalog.as_ref())
            .find_map(|catalog| {
                catalog
                    .textures()
                    .iter()
                    .find(|texture| texture.identifier.as_ref() == identifier)
            })?;
        Some(render_model::HandItemAtlas {
            width: texture.width,
            height: texture.height,
            layers: 1,
            rgba8: Arc::clone(&texture.rgba8),
        })
    }

    /// Registers two static planes once; pixel updates change only their image binding.
    pub(crate) fn map_meshes(&mut self) -> [EntityRigId; 2] {
        if let Some(meshes) = self.map_meshes {
            return meshes;
        }
        let meshes = std::array::from_fn(|index| {
            let id = item_mesh_rig_id(
                (MAX_ITEM_MESHES + session::MAX_SESSION_MESHES + index)
                    .try_into()
                    .expect("reserved map mesh IDs fit u32"),
            );
            let (min, max, z) = if index == 0 {
                (-7.0, 135.0, 0.0)
            } else {
                (0.0, protocol::MAP_IMAGE_SIDE as f32, -0.01)
            };
            let corners = [[min, min, z], [max, min, z], [max, max, z], [min, max, z]];
            let uv = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
            let vertices: Vec<_> = [3, 2, 1, 3, 1, 0]
                .map(|corner| render_model::ActorRigVertex {
                    position: corners[corner],
                    normal: [0.0, 0.0, -1.0],
                    uv: uv[corner],
                    back_uv: uv[corner],
                    bone_index: 0,
                    surface: render_model::ActorRigSurface::default(),
                    glint_origin: [0.0; 2],
                })
                .into();
            self.pending.push(
                ActorRigGeometry::new(id, vertices, vec![[0.0; 3]]).expect("finite map planes"),
            );
            id
        });
        self.map_meshes = Some(meshes);
        meshes
    }
}
