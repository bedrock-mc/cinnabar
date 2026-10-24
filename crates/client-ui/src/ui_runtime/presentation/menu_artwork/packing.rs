use std::{collections::HashMap, sync::Arc};

use {
    super::{Artwork, ArtworkSet, DecodeCache, Packed, Source, decode_bytes, sources},
    ui::IconRef,
    ui_engine::{
        pages::{ART_PAGE_GUTTER as GUTTER, WHOLE_ART_PAGE},
        textures::{MOD_MENU_TITLE_KEY, TITLE_KEY},
    },
};

const _: () = assert!(GUTTER > 0);

fn title() -> Option<&'static Artwork> {
    static TITLE: std::sync::OnceLock<Option<Artwork>> = std::sync::OnceLock::new();
    TITLE
        .get_or_init(|| {
            decode_bytes(launcher::branding::TITLE, WHOLE_ART_PAGE).map(
                |(pixels, width, height)| Artwork {
                    width,
                    height,
                    pixels,
                },
            )
        })
        .as_ref()
}

/// Shelf-packs ordinary art; full-width sources retain a dedicated page without edge gutters.
pub(super) fn pack(set: &ArtworkSet, cache: &DecodeCache, id: u64, complete: bool) -> Packed {
    let side = render_model::UI_ART_PAGE_SIDE;
    let rest: Vec<(String, &Artwork, bool)> = sources(set)
        .iter()
        .filter_map(|source| {
            let key = source.key();
            let art = cache.decoded.get(&key)?;
            Some((
                key.0,
                art.as_ref(),
                matches!(source, Source::Item(_, false)),
            ))
        })
        .collect();
    let decoded: Vec<(String, &Artwork, bool)> = title()
        .map(|art| (TITLE_KEY.to_owned(), art, false))
        .into_iter()
        .chain(
            set.logo
                .as_ref()
                .map(|logo| (MOD_MENU_TITLE_KEY.to_owned(), logo.as_ref(), false)),
        )
        .chain(rest)
        .collect();
    let page_bytes = side as usize * side as usize * 4;
    let mut buffers: Vec<Vec<u8>> = Vec::new();
    let mut refs = HashMap::with_capacity(decoded.len());
    let (mut page, mut x, mut y, mut shelf) = (None, GUTTER, GUTTER, 0u32);
    for (path, art, clamp_edges) in decoded {
        if art.width == 0
            || art.height == 0
            || art.width > side
            || art.height > side
            || art.pixels.len() != art.width as usize * art.height as usize * 4
        {
            continue;
        }
        if art.width + 2 * GUTTER > side || art.height + 2 * GUTTER > side {
            let dedicated = buffers.len();
            if dedicated >= render_model::MAX_UI_ART_PAGES {
                break;
            }
            buffers.push(vec![0; page_bytes]);
            copy_image(&mut buffers[dedicated], side, [0, 0], art, clamp_edges);
            refs.insert(path, icon(dedicated, [0, 0], art));
            continue;
        }
        if x + art.width + GUTTER > side {
            x = GUTTER;
            y += shelf + GUTTER;
            shelf = 0;
        }
        if y + art.height + GUTTER > side {
            page = None;
            (x, y, shelf) = (GUTTER, GUTTER, 0);
        }
        let target_page = match page {
            Some(page) => page,
            None => {
                let next = buffers.len();
                if next >= render_model::MAX_UI_ART_PAGES {
                    break;
                }
                buffers.push(vec![0; page_bytes]);
                page = Some(next);
                next
            }
        };
        copy_image(&mut buffers[target_page], side, [x, y], art, clamp_edges);
        refs.insert(path, icon(target_page, [x, y], art));
        x += art.width + GUTTER;
        shelf = shelf.max(art.height);
    }
    let pages = buffers
        .into_iter()
        .map(|pixels| {
            render_model::UiTexturePage::owned([side, side], Arc::from(pixels))
                .expect("art pages have exact checked dimensions")
        })
        .collect();
    Packed {
        id,
        complete,
        pages,
        refs,
    }
}

/// Copies texels and, for model textures, extends the far edges into their point-sampling gutter.
fn copy_image(page: &mut [u8], side: u32, [x, y]: [u32; 2], art: &Artwork, clamp_edges: bool) {
    let row_bytes = art.width as usize * 4;
    for row in 0..art.height as usize {
        let target = ((y as usize + row) * side as usize + x as usize) * 4;
        page[target..target + row_bytes]
            .copy_from_slice(&art.pixels[row * row_bytes..(row + 1) * row_bytes]);
        if clamp_edges && x + art.width < side {
            page[target + row_bytes..target + row_bytes + 4]
                .copy_from_slice(&art.pixels[(row + 1) * row_bytes - 4..(row + 1) * row_bytes]);
        }
    }
    if clamp_edges && y + art.height < side {
        let last_row = (art.height as usize - 1) * row_bytes;
        let target = (((y + art.height) * side + x) * 4) as usize;
        page[target..target + row_bytes]
            .copy_from_slice(&art.pixels[last_row..last_row + row_bytes]);
        if x + art.width < side {
            page[target + row_bytes..target + row_bytes + 4]
                .copy_from_slice(&art.pixels[last_row + row_bytes - 4..last_row + row_bytes]);
        }
    }
}

fn icon(page: usize, [x, y]: [u32; 2], art: &Artwork) -> IconRef {
    IconRef {
        color_mask: None,
        page: page as u16,
        uv: [
            x as u16,
            y as u16,
            (x + art.width) as u16,
            (y + art.height) as u16,
        ],
        glint: false,
    }
}

#[cfg(test)]
mod tests;
