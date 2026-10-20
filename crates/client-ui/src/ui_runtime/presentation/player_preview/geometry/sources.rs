//! Unprojected preview layers share one topology for admission and drawing.

use std::{borrow::Cow, sync::Arc};

use glam::Affine3A;
use render_model::{ActorVertex, standard_biped_overlay_vertices, standard_biped_vertices};
use ui::{IconRef, UiBlendMode, UiMesh, UiMeshBatch, UiMeshVertex};

use super::{PreviewEquipment, PreviewFire, equipment};

pub(super) struct Layer<'a> {
    pub(super) vertices: Cow<'a, [ActorVertex]>,
    pub(super) icon: IconRef,
    pub(super) tint: Option<[u8; 3]>,
}

/// Source layers are prepared once, counted without projection, then drawn with admitted hands.
pub(in crate::ui_runtime::presentation) struct MeshSources<'a> {
    pub(super) layers: Vec<Layer<'a>>,
    pub(super) parts: [Option<Affine3A>; 6],
    pub(super) fire: Option<PreviewFire>,
    pub(super) particles: Option<&'a UiMesh>,
}

impl<'a> MeshSources<'a> {
    /// Resolves the ordered skin, cape and armor sources without creating UI vertices.
    pub(in crate::ui_runtime::presentation) fn new(
        body: Option<(&'a [ActorVertex], &[Option<Affine3A>; 6])>,
        skin: IconRef,
        gear: &PreviewEquipment,
        armor: [Option<IconRef>; 4],
        fire: Option<PreviewFire>,
        cape: Option<(&'a [ActorVertex], IconRef)>,
    ) -> Self {
        let vertices = body.map_or_else(
            || {
                let mut vertices = standard_biped_vertices();
                vertices.extend(standard_biped_overlay_vertices());
                Cow::Owned(vertices)
            },
            |(vertices, _)| Cow::Borrowed(vertices),
        );
        let mut layers = vec![Layer {
            vertices,
            icon: skin,
            tint: None,
        }];
        if let Some((vertices, icon)) = cape {
            layers.push(Layer {
                vertices: Cow::Borrowed(vertices),
                icon,
                tint: None,
            });
        }
        for (slot, icon) in armor.into_iter().enumerate() {
            if let (Some(icon), Some(texture)) = (icon, &gear.armor[slot]) {
                layers.push(Layer {
                    vertices: Cow::Owned(equipment::armor_vertices(slot)),
                    icon,
                    tint: texture.tint,
                });
            }
        }
        Self {
            layers,
            parts: body.map_or([None; 6], |(_, parts)| *parts),
            fire,
            particles: None,
        }
    }

    /// Adds an independently textured head attachment to the shared depth pass.
    pub(in crate::ui_runtime::presentation) fn with_attachment(
        mut self,
        vertices: &'a [ActorVertex],
        icon: IconRef,
    ) -> Self {
        self.layers.push(Layer {
            vertices: Cow::Borrowed(vertices),
            icon,
            tint: None,
        });
        self
    }

    /// Adds already projected particle quads to the player's isolated depth pass.
    pub(in crate::ui_runtime::presentation) fn with_particles(
        mut self,
        particles: Option<&'a UiMesh>,
    ) -> Self {
        self.particles = particles;
        self
    }

    /// Counts triangle corners and batches from the same layers the projection consumes.
    pub(in crate::ui_runtime::presentation) fn counts(&self) -> Option<(usize, usize)> {
        let mut vertices = 0usize;
        for layer in &self.layers {
            vertices = vertices.checked_add(layer.vertices.len())?;
        }
        let mut batches = self.layers.len();
        if let Some(fire) = self.fire {
            vertices = vertices.checked_add(fire.vertex_count()?)?;
            batches = batches.checked_add(1)?;
        }
        if let Some(particles) = self.particles {
            vertices = vertices.checked_add(particles.vertices().len())?;
            batches = batches.checked_add(particles.batches().len())?;
        }
        Some((vertices, batches))
    }

    /// Represents only batch boundaries for a clipping/coalescing probe, never for display.
    pub(in crate::ui_runtime::presentation) fn batch_probe(&self) -> Option<Arc<UiMesh>> {
        let (_, batches) = self.counts()?;
        let indices = batches.checked_mul(3)?;
        let end = u32::try_from(indices).ok()?;
        let texture_page = self.layers.first()?.icon.page;
        let vertex = UiMeshVertex {
            position: [0.0; 2],
            clip_z: 0.5,
            clip_w: 1.0,
            uv: [0.0; 2],
            color: [255; 4],
            model_light: 1.0,
            overlay_color: [0.0; 4],
            style_flags: 0,
            alpha_test: false,
        };
        let materials = (0..end)
            .step_by(3)
            .map(|start| UiMeshBatch {
                texture_page,
                index_range: start..start + 3,
                blend: UiBlendMode::Alpha,
                depth_test: true,
                depth_write: true,
                alpha_cutoff: Some(0.5),
            })
            .collect::<Vec<_>>();
        UiMesh::new(
            Arc::from([vertex]),
            vec![0; indices].into(),
            materials.into(),
        )
        .ok()
        .map(Arc::new)
    }
}
