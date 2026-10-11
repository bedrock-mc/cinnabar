use std::{collections::HashMap, sync::Arc};

use {
    super::{
        Artwork, ArtworkSet, DecodeCache, GUTTER, Packed, TITLE_KEY, WHOLE_PAGE, decode_bytes,
        sources,
    },
    ui::IconRef,
};

fn title() -> Option<&'static Artwork> {
    static TITLE: std::sync::OnceLock<Option<Artwork>> = std::sync::OnceLock::new();
    TITLE
        .get_or_init(|| {
            decode_bytes(launcher::branding::TITLE, WHOLE_PAGE).map(|(pixels, width, height)| {
                Artwork {
                    width,
                    height,
                    pixels,
                }
            })
        })
        .as_ref()
}

/// Shelf-packs ordinary art; full-width sources retain a dedicated page without edge gutters.
pub(super) fn pack(set: &ArtworkSet, cache: &DecodeCache, id: u64, complete: bool) -> Packed {
    let side = render_model::UI_ART_PAGE_SIDE;
    let rest: Vec<(String, &Artwork)> = sources(set)
        .iter()
        .filter_map(|source| {
            let key = source.key();
            let art = cache.decoded.get(&key)?;
            Some((key.0, art.as_ref()))
        })
        .collect();
    let decoded: Vec<(String, &Artwork)> = title()
        .map(|art| (TITLE_KEY.to_owned(), art))
        .into_iter()
        .chain(rest)
        .collect();
    let page_bytes = side as usize * side as usize * 4;
    let mut buffers: Vec<Vec<u8>> = Vec::new();
    let mut refs = HashMap::with_capacity(decoded.len());
    let (mut page, mut x, mut y, mut shelf) = (None, GUTTER, GUTTER, 0u32);
    for (path, art) in decoded {
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
            copy_image(&mut buffers[dedicated], side, [0, 0], art);
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
        copy_image(&mut buffers[target_page], side, [x, y], art);
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

fn copy_image(page: &mut [u8], side: u32, [x, y]: [u32; 2], art: &Artwork) {
    let row_bytes = art.width as usize * 4;
    for row in 0..art.height as usize {
        let target = ((y as usize + row) * side as usize + x as usize) * 4;
        page[target..target + row_bytes]
            .copy_from_slice(&art.pixels[row * row_bytes..(row + 1) * row_bytes]);
    }
}

fn icon(page: usize, [x, y]: [u32; 2], art: &Artwork) -> IconRef {
    IconRef {
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
