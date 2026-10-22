//! Page sizing: same-size rasters share capped array pages within the actor pixel budget.

use super::{
    ActorArtworkPageId, ActorTexturePage, MAX_ACTOR_GPU_PIXEL_BYTES, MAX_ACTOR_TEXTURE_PAGES,
};

/// Layers per generic entity page, within every backend's array-layer limit.
pub(super) const MAX_ACTOR_PAGE_LAYERS: usize = 256;
/// One page is one GPU allocation; large rasters split so a failed or replaced page stays small.
pub(super) const MAX_ACTOR_PAGE_BYTES: usize = 32 * 1024 * 1024;

/// Layers per page for rasters of one size; a raster larger than the cap gets its own page.
pub(super) fn page_layers(width: u16, height: u16) -> usize {
    let layer = usize::from(width) * usize::from(height) * 4;
    (MAX_ACTOR_PAGE_BYTES / layer.max(1)).clamp(1, MAX_ACTOR_PAGE_LAYERS)
}

pub(super) fn within_page_budget(generic_pages: usize, declared_pixel_bytes: usize) -> bool {
    generic_pages < MAX_ACTOR_TEXTURE_PAGES && declared_pixel_bytes <= MAX_ACTOR_GPU_PIXEL_BYTES
}

/// Appends `page`, box-filtered down until it fits the byte budget, and returns its page id;
/// `None` only once every page id is taken.
pub(super) fn push_page(
    pages: &mut Vec<ActorTexturePage>,
    gpu_bytes: &mut usize,
    page: ActorTexturePage,
) -> Option<ActorArtworkPageId> {
    let mut page = page;
    while !within_page_budget(pages.len() + 1, gpu_bytes.saturating_add(page.rgba8.len())) {
        let longest = u32::from(page.width.max(page.height));
        if pages.len() + 1 >= MAX_ACTOR_TEXTURE_PAGES || longest <= 1 {
            return None;
        }
        page = page.fit_within(longest / 2).into_owned();
    }
    *gpu_bytes += page.rgba8.len();
    pages.push(page);
    ActorArtworkPageId::try_from(pages.len()).ok()
}

/// Pushes a same-size group split into capped pages, all at the one resolution the remaining
/// budget allows for the whole group; returns each chunk with its page id (`None` once ids run out).
pub(super) fn push_group<'a>(
    pages: &mut Vec<ActorTexturePage>,
    gpu_bytes: &mut usize,
    members: &'a [usize],
    [width, height]: [u16; 2],
    multitexture: bool,
    mut build: impl FnMut(&'a [usize]) -> ActorTexturePage,
) -> Vec<(&'a [usize], Option<ActorArtworkPageId>)> {
    let mut limit = u32::from(width.max(height));
    let [fitted_width, fitted_height] = loop {
        let fitted = crate::actor::texel_fit::fitted_dimensions(width, height, limit);
        let bytes = usize::from(fitted[0]) * usize::from(fitted[1]) * 4 * members.len();
        if gpu_bytes.saturating_add(bytes) <= MAX_ACTOR_GPU_PIXEL_BYTES || limit <= 1 {
            break fitted;
        }
        limit /= 2;
    };
    // Three-sampler layers must share a page, so those keep the layer limit alone.
    let layers = if multitexture {
        MAX_ACTOR_PAGE_LAYERS
    } else {
        page_layers(fitted_width, fitted_height)
    };
    members
        .chunks(layers)
        .map(|chunk| {
            let page = build(chunk).fit_within(limit).into_owned();
            (chunk, push_page(pages, gpu_bytes, page))
        })
        .collect()
}
