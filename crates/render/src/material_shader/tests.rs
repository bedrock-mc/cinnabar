use crate::material_shader::*;

#[test]
fn gpu_metadata_preserves_page_and_layer() {
    let mask = assets::TEXTURE_LAYER_MASK;
    for page in 0..assets::MAX_TEXTURE_PAGES as u32 {
        for layer in [1, 2049, mask] {
            for dimensions in [[32, 32], [8, 16], [256, 128]] {
                for grid in [0, 1, 3] {
                    let reference = assets::TextureRef::new(page, layer).unwrap();
                    let encoded = gpu_grid_texture_ref(reference, dimensions, 256, grid);
                    assert_eq!(encoded & mask, layer, "{dimensions:?}, grid {grid}");
                    assert_eq!(encoded >> 31, page);
                }
            }
        }
    }
}
