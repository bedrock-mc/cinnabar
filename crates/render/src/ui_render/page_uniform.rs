//! Per-layer source ownership for original images split across texture pages.

use render_model::{MAX_UI_TEXTURE_LAYERS, UiTexturePage};

const HEADER_WORDS: usize = 4;
const REGION_WORDS: usize = 8;
pub(super) const BYTE_SIZE: u64 =
    ((HEADER_WORDS + REGION_WORDS * MAX_UI_TEXTURE_LAYERS as usize) * size_of::<u32>()) as u64;

/// Initializes the shared format header and bounded per-layer region table.
pub(super) fn words(coverage: bool) -> Vec<u32> {
    let mut words = vec![0; BYTE_SIZE as usize / size_of::<u32>()];
    words[0] = u32::from(coverage);
    words
}

/// Byte offset of a resident layer's source-ownership entry.
pub(super) fn offset(layer: u32) -> u64 {
    ((HEADER_WORDS + REGION_WORDS * layer as usize) * size_of::<u32>()) as u64
}

/// The source texel bounds and actual page dimensions, excluding any bucket padding.
pub(super) fn region(page: &UiTexturePage) -> [f32; REGION_WORDS] {
    let [w, h] = page.dimensions();
    let bounds = page.repeat_content().unwrap_or([0, 0, w, h]);
    [
        bounds[0] as f32,
        bounds[1] as f32,
        bounds[2] as f32,
        bounds[3] as f32,
        w as f32,
        h as f32,
        0.0,
        0.0,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeat_page_table_matches_the_shader_uniform_layout() {
        let text = super::super::shader::source(include_str!("../ui.wgsl"));
        let module = naga::front::wgsl::parse_str(&text).expect("UI shader parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .expect("UI shader validates");
        let (_, layout) = module
            .types
            .iter()
            .find(|(_, ty)| ty.name.as_deref() == Some("PageFormat"))
            .unwrap();
        let naga::TypeInner::Struct { members, span } = &layout.inner else {
            panic!("page format is a uniform struct");
        };
        assert_eq!(u64::from(*span), BYTE_SIZE);
        assert_eq!(u64::from(members[1].offset), offset(0));
        assert_eq!(words(false).len() * size_of::<u32>(), BYTE_SIZE as usize);
    }
}
