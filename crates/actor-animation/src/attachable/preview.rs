//! Exact provisional attachable evaluations, committed only after the drawn hand is selected.
use super::*;

pub(super) struct Preview {
    pub(super) evaluated: EvaluatedState,
    pub(super) tick: u64,
    pub(super) geometry: Option<geometry::GeometryCheckpoint>,
}

impl std::fmt::Debug for Preview {
    /// Reports provisional state without formatting the retained VM and pose buffers.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Preview")
            .field("tick", &self.tick)
            .finish_non_exhaustive()
    }
}

impl AttachableState {
    /// Restores moved geometry before a changed hand reevaluates the original committed state.
    pub(super) fn discard_preview(&mut self) {
        if let Some(Preview {
            geometry: Some(geometry),
            ..
        }) = self.preview.take()
        {
            geometry.restore(&mut self.rig);
        }
    }

    /// Adopts a provisional result without rerunning authored scripts or clocks.
    pub(super) fn commit_preview(&mut self) -> bool {
        let Some(preview) = self.preview.take() else {
            return false;
        };
        self.commit(preview.evaluated, preview.tick);
        true
    }

    /// Applies the dynamic result exactly once while geometry remains owned by the same state.
    pub(super) fn commit(&mut self, evaluated: EvaluatedState, tick: u64) -> bool {
        let state = &mut self.rig;
        state.variables = evaluated.variables;
        state.controllers = evaluated.controllers;
        state.server_animations = evaluated.server_animations;
        state.clip_clocks = evaluated.clip_clocks;
        state.current = evaluated.pose;
        state.scale = evaluated.scale;
        let Some(render) = evaluated.render else {
            return false;
        };
        state.render = render;
        state.initialized = true;
        state.reset_pending = false;
        state.completed_tick = tick;
        true
    }

    /// Borrows the exact provisional draw or its committed result without copying pose buffers.
    pub(super) fn snapshot(
        &self,
        assets: &RuntimeEntityAssets,
        binding: usize,
    ) -> Option<AttachableRigSnapshot<'_>> {
        let state = &self.rig;
        let (pose, render, scale) = match &self.preview {
            Some(preview) => (
                &preview.evaluated.pose,
                preview.evaluated.render.as_deref()?,
                preview.evaluated.scale,
            ),
            None => (&state.current, state.render.as_slice(), state.scale),
        };
        Some(AttachableRigSnapshot {
            geometry: assets
                .rig_geometries()
                .get(state.geometry_binding)?
                .geometry,
            pose,
            bone_names: &state.bone_names,
            render,
            scale: scale.map_or(assets.rig_bindings()[binding].scale.get(), |s| s[0]),
            axis_scale: scale.map_or([1.0; 3], |s| [s[1], s[2], s[3]]),
            bones: &state.bones,
            layer_skeletons: &state.layer_skeletons,
        })
    }
}
