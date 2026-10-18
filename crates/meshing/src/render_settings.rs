/// World geometry choices captured with each mesh job and resource-pack rebuild.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeshRenderSettings {
    /// Interpolate light and ambient occlusion at terrain vertices.
    pub smooth_lighting: bool,
    /// Draw leaf holes and the exposed, two-sided leaf layers.
    pub transparent_leaves: bool,
}

impl Default for MeshRenderSettings {
    /// Keeps the full-quality world rendering used by existing callers.
    fn default() -> Self {
        Self {
            smooth_lighting: true,
            transparent_leaves: true,
        }
    }
}
