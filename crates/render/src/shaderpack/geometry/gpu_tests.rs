use super::*;
use std::sync::Arc;

#[test]
fn expansion_sources_must_share_carrier_revision_and_authored_bindings() {
    let selected =
        ChunkTextureAssets::with_revision(Arc::new(assets::RuntimeAssets::diagnostic()), 1);
    let newer = ChunkTextureAssets::with_revision(Arc::clone(selected.assets()), 2);
    let other = ChunkTextureAssets::with_revision(Arc::new(assets::RuntimeAssets::diagnostic()), 1);
    assert!(expansion_sources_match(
        &selected,
        &selected.clone(),
        selected.identity()
    ));
    assert!(!expansion_sources_match(
        &newer,
        &selected,
        newer.identity()
    ));
    assert!(!expansion_sources_match(
        &other,
        &selected,
        other.identity()
    ));
    assert!(!expansion_sources_match(
        &selected,
        &selected,
        newer.identity()
    ));
    let page = || assets::TextureArray {
        layers: 0,
        mips: Box::new([]),
    };
    let authored =
        selected.with_updated_enhanced(Some(Arc::new(render_scene::EnhancedTextureAssets {
            color_pages: [page(), page()],
            normal_pages: [page(), page()],
            mer_pages: [page(), page()],
            texture_refs: Box::new([]),
        })));
    assert!(!expansion_sources_match(
        &authored,
        &selected,
        selected.identity()
    ));
    assert!(expansion_sources_match(
        &authored,
        &authored.clone(),
        selected.identity()
    ));
}

#[test]
fn release_all_drops_retained_expansion_buffers() {
    let Some(gpu) = crate::gpu_snapshot::Gpu::new() else {
        return;
    };
    let device = RenderDevice::from(gpu.device);
    let queue = RenderQueue::new(gpu.queue);
    let mut geometry = ShaderpackGeometry::new(&device, Some(false));
    geometry
        .expansion
        .write(&device, &queue, &[0, 0], [0; PARAM_WORDS], &[&[[0; 4]]]);
    assert!(geometry.expansion.jobs().is_some());
    assert!(geometry.expansion.params(0).is_some());
    geometry.release_all();
    assert!(geometry.expansion.jobs().is_none());
    assert!(geometry.expansion.params(0).is_none());
    assert!(geometry.inputs.is_none());
    assert!(geometry.views.is_empty() && geometry.shadows.is_empty());
}
