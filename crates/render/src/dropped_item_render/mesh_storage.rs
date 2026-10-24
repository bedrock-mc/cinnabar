//! Aggregate vertex admission for the dropped-item upload.

use render_model::ItemMeshVertex;
use std::ops::Deref;

/// Retained meshes bounded by both the device buffer and shared storage policy.
pub(super) struct MeshStorage {
    vertices: Vec<ItemMeshVertex>,
    max_vertices: usize,
}

impl MeshStorage {
    /// Allows UV adjustment within already admitted storage without changing its length.
    pub(super) fn vertices_mut(&mut self) -> &mut [ItemMeshVertex] {
        &mut self.vertices
    }

    /// Uses the complete vertex limit already calculated by shared admission.
    pub(super) fn with_capacity_limit(max_vertices: usize) -> Self {
        Self {
            vertices: Vec::new(),
            max_vertices,
        }
    }

    /// Whether this mesh could fit if all other admitted geometry were removed.
    pub(super) fn fits_empty(&self, vertices: usize) -> bool {
        vertices <= self.max_vertices
    }

    /// Appends an entire mesh if it fits, leaving storage unchanged on rejection.
    pub(super) fn append(&mut self, mesh: Vec<ItemMeshVertex>) -> bool {
        if mesh.len() > self.max_vertices.saturating_sub(self.vertices.len()) {
            return false;
        }
        let required = self.vertices.len() + mesh.len();
        if required > self.vertices.capacity() {
            let capacity = required
                .max(self.vertices.capacity().saturating_mul(2))
                .min(self.max_vertices);
            self.vertices.reserve_exact(capacity - self.vertices.len());
        }
        self.vertices.extend(mesh);
        true
    }
}

impl Deref for MeshStorage {
    type Target = [ItemMeshVertex];

    /// Exposes only admitted vertices for bounds calculation and upload.
    fn deref(&self) -> &Self::Target {
        &self.vertices
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use render_model::dropped_item::ITEM_MESH_VERTEX_BYTES;

    /// Creates an original vertex without relying on device state.
    fn vertex() -> ItemMeshVertex {
        ItemMeshVertex {
            position: [0.; 3],
            uv: [0.; 2],
            normal: [0., 1., 0.],
            layer: 0,
            color: render_model::OPAQUE_WHITE,
            glint_scale: [0.0; 2],
        }
    }

    #[test]
    fn aggregate_mesh_admission_respects_a_smaller_device_buffer() {
        let budget = render_model::dropped_item::ItemModelBudget::new(
            assets::texture::policy::device_policy(),
            render_model::MAX_ITEM_LAYERS,
            (ITEM_MESH_VERTEX_BYTES * 3 - 1) as u64,
        );
        let mut storage = MeshStorage::with_capacity_limit(budget.max_vertices());
        assert!(storage.append(vec![vertex()]));
        assert!(!storage.append(vec![vertex(); 2]));
        assert_eq!(storage.len(), 1);
        assert!(storage.append(vec![vertex()]));
        assert_eq!(storage.len(), 2);
    }
}
