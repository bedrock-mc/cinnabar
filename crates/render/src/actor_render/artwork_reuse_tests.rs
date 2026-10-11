use super::*;

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
    pages.pages = Arc::from([crate::ActorTexturePage {
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
    let base = gpu.pages[0]._texture.id();
    let glint = gpu.glint.as_ref().unwrap().0.id();
    let (extended, _) = original
        .clone()
        .with_equipment_rasters(&[crate::EquipmentRaster {
            width: 8,
            height: 8,
            rgba8: vec![31; 8 * 8 * 4].into(),
        }]);
    assert!(gpu.prepare(&extended, &device, &queue));
    assert_eq!(gpu.pages.len(), 2);
    assert_eq!(
        gpu.pages[0]._texture.id(),
        base,
        "unchanged pages must not allocate or upload again"
    );
    assert_eq!(gpu.glint.as_ref().unwrap().0.id(), glint);
    assert!(gpu.prepare(&original, &device, &queue));
    assert_eq!(gpu.pages.len(), 1);
    assert_eq!(gpu.pages[0]._texture.id(), base);
}

#[test]
fn route_and_page_order_changes_reuse_immutable_textures() {
    let (device, queue) = gpu();
    let (mut pages, _) = artwork().with_equipment_rasters(&[crate::EquipmentRaster {
        width: 8,
        height: 8,
        rgba8: vec![31; 8 * 8 * 4].into(),
    }]);
    let mut gpu = GpuArtwork::default();
    assert!(gpu.prepare(&pages, &device, &queue));
    let ids: Vec<_> = gpu.pages.iter().map(|page| page._texture.id()).collect();
    pages.entity_identity = [3; 32];
    pages.pages = pages.pages.iter().rev().cloned().collect();
    assert!(gpu.prepare(&pages, &device, &queue));
    assert_eq!(gpu.pages[0]._texture.id(), ids[1]);
    assert_eq!(gpu.pages[1]._texture.id(), ids[0]);
}

#[test]
fn equal_pixels_in_independent_allocations_reuse_textures_after_material_changes() {
    let (device, queue) = gpu();
    let mut pages = artwork();
    let mut gpu = GpuArtwork::default();
    assert!(gpu.prepare(&pages, &device, &queue));
    let texture = gpu.pages[0]._texture.id();
    let glint = gpu.glint.as_ref().unwrap().0.id();
    let mut page = pages.pages[0].clone();
    page.rgba8 = Arc::from(page.rgba8.to_vec());
    page.color_mask = true;
    pages.pages = Arc::from([page]);
    pages.entity_identity = [7; 32];
    assert!(gpu.prepare(&pages, &device, &queue));
    assert_eq!(gpu.pages[0]._texture.id(), texture);
    assert!(gpu.pages[0].color_mask);
    assert_eq!(gpu.glint.as_ref().unwrap().0.id(), glint);
}

#[test]
fn changed_glint_and_pixels_invalidate_only_their_own_texture() {
    let (device, queue) = gpu();
    let mut pages = artwork();
    let mut gpu = GpuArtwork::default();
    assert!(gpu.prepare(&pages, &device, &queue));
    let texture = gpu.pages[0]._texture.id();
    let glint = gpu.glint.as_ref().unwrap().0.id();
    pages = pages.with_actor_glint(crate::EquipmentRaster {
        width: 1,
        height: 1,
        rgba8: Arc::from([19; 4]),
    });
    assert!(gpu.prepare(&pages, &device, &queue));
    assert_eq!(gpu.pages[0]._texture.id(), texture);
    assert_ne!(gpu.glint.as_ref().unwrap().0.id(), glint);
    let changed_glint = gpu.glint.as_ref().unwrap().0.id();
    let mut page = pages.pages[0].clone();
    page.rgba8 = vec![43; page.rgba8.len()].into();
    pages.pages = Arc::from([page]);
    pages.entity_identity = [9; 32];
    assert!(gpu.prepare(&pages, &device, &queue));
    assert_ne!(gpu.pages[0]._texture.id(), texture);
    assert_eq!(gpu.glint.as_ref().unwrap().0.id(), changed_glint);
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
    assert!(gpu.glint.is_some());
}
