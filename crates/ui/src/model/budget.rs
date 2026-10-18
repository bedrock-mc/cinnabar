//! Shared visual reservations for retained UI frames and prospective model replacements.

use std::mem::size_of;

use super::{TextShadow, UiDrawBatch, UiError, UiLimits, UiVertex, UiVisual};

/// A node's share of the draw list's reserved capacity: quads, and mesh vertices, indices and
/// batches.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct DrawCounts {
    pub(super) quads: usize,
    pub(super) mesh_vertices: usize,
    pub(super) mesh_indices: usize,
    pub(super) mesh_batches: usize,
}

impl DrawCounts {
    /// What one visual reserves, before clipping drops any of it.
    pub(super) fn of(visual: &UiVisual) -> Result<Self, UiError> {
        let quads = match visual {
            UiVisual::None => 0,
            UiVisual::Mesh(mesh) => {
                return Ok(Self {
                    quads: 0,
                    mesh_vertices: mesh.draw_vertex_count(),
                    mesh_indices: mesh.indices().len(),
                    mesh_batches: mesh.batches().len(),
                });
            }
            UiVisual::Solid { .. }
            | UiVisual::Sprite { .. }
            | UiVisual::GlintSprite { .. }
            | UiVisual::StyledSprite { .. }
            | UiVisual::Gradient { .. }
            | UiVisual::RotatedSprite { .. }
            | UiVisual::InvertedSprite { .. } => 1,
            UiVisual::Text { layout, shadow, .. }
            | UiVisual::RotatedText { layout, shadow, .. } => {
                let passes = match shadow {
                    TextShadow::None => 1,
                    TextShadow::Offset64(_) => 2,
                };
                // A bold glyph emits a second, offset copy per pass.
                let bold = layout
                    .glyphs()
                    .iter()
                    .filter(|glyph| glyph.style.bold)
                    .count();
                layout
                    .glyphs()
                    .len()
                    .checked_add(bold)
                    .and_then(|per_pass| per_pass.checked_mul(passes))
                    .ok_or(UiError::DrawIndexOverflow)?
            }
        };
        Ok(Self {
            quads,
            ..Self::default()
        })
    }

    /// Both shares together.
    pub(super) fn add(self, other: Self) -> Result<Self, UiError> {
        let sum =
            |left: usize, right: usize| left.checked_add(right).ok_or(UiError::DrawIndexOverflow);
        Ok(Self {
            quads: sum(self.quads, other.quads)?,
            mesh_vertices: sum(self.mesh_vertices, other.mesh_vertices)?,
            mesh_indices: sum(self.mesh_indices, other.mesh_indices)?,
            mesh_batches: sum(self.mesh_batches, other.mesh_batches)?,
        })
    }

    /// The list's `(quads and mesh batches, vertices, indices)` capacity, rejecting a frame
    /// beyond the vertex or index limit.
    fn reserved(self) -> Result<(usize, usize, usize), UiError> {
        let vertices = self
            .quads
            .checked_mul(4)
            .and_then(|count| count.checked_add(self.mesh_vertices))
            .ok_or(UiError::DrawIndexOverflow)?;
        if vertices > UiLimits::MAX_UI_VERTICES {
            return Err(UiError::VertexLimitExceeded {
                actual: vertices,
                limit: UiLimits::MAX_UI_VERTICES,
            });
        }
        let indices = self
            .quads
            .checked_mul(6)
            .and_then(|count| count.checked_add(self.mesh_indices))
            .ok_or(UiError::DrawIndexOverflow)?;
        if indices > UiLimits::MAX_UI_INDICES {
            return Err(UiError::IndexLimitExceeded {
                actual: indices,
                limit: UiLimits::MAX_UI_INDICES,
            });
        }
        Ok((
            self.quads
                .checked_add(self.mesh_batches)
                .ok_or(UiError::DrawIndexOverflow)?,
            vertices,
            indices,
        ))
    }

    /// Conservative batch count before clipping and quad coalescing.
    pub(super) fn batch_upper_bound(self) -> Result<usize, UiError> {
        self.reserved().map(|(batches, _, _)| batches)
    }

    /// Checks the whole list's reservation against the draw-list byte limit, returning the
    /// `(batch, vertex, index)` capacities to reserve.
    pub(super) fn budget(self) -> Result<(usize, usize, usize), UiError> {
        let (batch_count, vertex_count, index_count) = self.reserved()?;
        // Clipping can drop meshes; emission enforces the actual batch limit.
        let batch_capacity = batch_count.min(UiLimits::MAX_DRAW_BATCHES);
        let reserved_bytes = vertex_count
            .checked_mul(size_of::<UiVertex>())
            .and_then(|bytes| {
                index_count
                    .checked_mul(size_of::<u32>())
                    .and_then(|index_bytes| bytes.checked_add(index_bytes))
            })
            .and_then(|bytes| {
                batch_capacity
                    .checked_mul(size_of::<UiDrawBatch>())
                    .and_then(|batch_bytes| bytes.checked_add(batch_bytes))
            })
            .ok_or(UiError::DrawIndexOverflow)?;
        if reserved_bytes > UiLimits::MAX_DRAW_LIST_BYTES {
            return Err(UiError::DrawByteLimitExceeded {
                actual: reserved_bytes,
                limit: UiLimits::MAX_DRAW_LIST_BYTES,
            });
        }
        Ok((batch_capacity, vertex_count, index_count))
    }
}
