use super::*;
use render_model::ActorArtworkPages;

/// Creates a no-op GPU so resource identity checks are deterministic on every CI platform.
fn gpu() -> (RenderDevice, RenderQueue) {
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    (RenderDevice::from(device), RenderQueue::new(queue))
}

/// One retained source page to which a publication may append equipment textures.
fn artwork() -> ActorArtworkPages {
    let mut pages = ActorArtworkPages::default();
    pages.identity = [1; 32];
    pages.entity_identity = [2; 32];
    pages.pages = Arc::from([render_model::ActorTexturePage {
        width: 16,
        height: 16,
        layers: 1,
        rgba8: vec![7; 16 * 16 * 4].into(),
        color_mask: false,
        multitexture: false,
    }]);
    pages
}

#[test]
fn publication_of_one_new_page_retains_existing_textures_and_glint() {
    let (device, queue) = gpu();
    let original = artwork();
    let mut gpu = GpuArtwork::default();
    assert!(gpu.prepare(&original, &device, &queue));
    let base = gpu.page(0).unwrap()._texture.id();
    let glint = gpu.glint.texture.as_ref().unwrap().0.id();
    let (extended, _) = original
        .clone()
        .with_equipment_rasters(&[render_model::EquipmentRaster {
            width: 8,
            height: 8,
            rgba8: vec![31; 8 * 8 * 4].into(),
        }]);
    assert!(gpu.prepare(&extended, &device, &queue));
    assert_eq!(gpu.pages.len(), 2);
    assert_eq!(
        gpu.page(0).unwrap()._texture.id(),
        base,
        "unchanged pages must not allocate or upload again"
    );
    assert_eq!(gpu.glint.texture.as_ref().unwrap().0.id(), glint);
    assert!(gpu.prepare(&original, &device, &queue));
    assert_eq!(gpu.pages.len(), 1);
    assert_eq!(gpu.page(0).unwrap()._texture.id(), base);
}

#[test]
fn equal_pixels_in_the_same_slot_reuse_its_texture_after_material_changes() {
    let (device, queue) = gpu();
    let mut pages = artwork();
    let mut gpu = GpuArtwork::default();
    assert!(gpu.prepare(&pages, &device, &queue));
    let texture = gpu.page(0).unwrap()._texture.id();
    let glint = gpu.glint.texture.as_ref().unwrap().0.id();
    let mut page = pages.pages[0].clone();
    page.rgba8 = Arc::from(page.rgba8.to_vec());
    page.color_mask = true;
    pages.pages = Arc::from([page]);
    pages.entity_identity = [7; 32];
    assert!(gpu.prepare(&pages, &device, &queue));
    assert_eq!(gpu.page(0).unwrap()._texture.id(), texture);
    assert!(gpu.page(0).unwrap().color_mask);
    assert_eq!(gpu.glint.texture.as_ref().unwrap().0.id(), glint);
}

#[test]
fn changed_glint_and_pixels_invalidate_only_their_own_texture() {
    let (device, queue) = gpu();
    let mut pages = artwork();
    let mut gpu = GpuArtwork::default();
    assert!(gpu.prepare(&pages, &device, &queue));
    let texture = gpu.page(0).unwrap()._texture.id();
    let glint = gpu.glint.texture.as_ref().unwrap().0.id();
    pages = pages.with_actor_glint(render_model::EquipmentRaster {
        width: 1,
        height: 1,
        rgba8: Arc::from([19; 4]),
    });
    assert!(gpu.prepare(&pages, &device, &queue));
    assert_eq!(gpu.page(0).unwrap()._texture.id(), texture);
    assert_ne!(gpu.glint.texture.as_ref().unwrap().0.id(), glint);
    let changed_glint = gpu.glint.texture.as_ref().unwrap().0.id();
    let mut page = pages.pages[0].clone();
    page.rgba8 = vec![43; page.rgba8.len()].into();
    pages.pages = Arc::from([page]);
    pages.entity_identity = [9; 32];
    assert!(gpu.prepare(&pages, &device, &queue));
    assert_ne!(gpu.page(0).unwrap()._texture.id(), texture);
    assert_eq!(gpu.glint.texture.as_ref().unwrap().0.id(), changed_glint);
}

#[test]
fn rejected_generic_pages_keep_glint_available_for_player_bindings() {
    let (device, queue) = gpu();
    let mut pages = artwork();
    let mut page = pages.pages[0].clone();
    page.layers = device.limits().max_texture_array_layers + 1;
    pages.pages = Arc::from([page]);
    let mut gpu = GpuArtwork::default();
    assert!(!gpu.prepare(&pages, &device, &queue));
    assert!(gpu.pages.is_empty());
    assert!(gpu.glint.texture.is_some());
}

/// Pages and glints allow the pack's raw view only while a pack draws; switching recreates them.
#[cfg(feature = "shaderpacks")]
#[test]
fn artwork_allows_raw_views_only_while_a_pack_draws() {
    use crate::actor::gpu::{ArtworkRawViews, permits_raw_view};
    let (device, queue) = gpu();
    let pages = artwork();
    let mut gpu = GpuArtwork::default();
    assert!(gpu.prepare(&pages, &device, &queue));
    let raw = |gpu: &GpuArtwork| {
        let page = gpu.page(0).unwrap();
        assert_eq!(
            permits_raw_view(device.wgpu_device(), &page._texture),
            page.raw_texture().is_some()
        );
        [
            page.raw_texture().is_some(),
            gpu.glint.raw_texture().is_some(),
            gpu.item_glint.raw_texture().is_some(),
        ]
    };
    assert_eq!(
        raw(&gpu),
        [false; 3],
        "Vanilla artwork keeps one view format"
    );
    for pack in [true, false] {
        gpu.set_raw_views(ArtworkRawViews(pack));
        assert!(gpu.prepare(&pages, &device, &queue));
        assert_eq!(raw(&gpu), [pack; 3]);
    }
}
