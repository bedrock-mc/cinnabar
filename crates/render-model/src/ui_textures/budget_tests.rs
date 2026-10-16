use super::*;
use crate::MAX_UI_FIXED_TEXTURE_BYTES;

fn rgba(dimensions: [u32; 2]) -> UiTexturePage {
    UiTexturePage::owned(
        dimensions,
        vec![0; (dimensions[0] * dimensions[1] * 4) as usize].into(),
    )
    .unwrap()
}

fn near_fixed_limit() -> UiTextureCatalog {
    let small = rgba([UI_DYNAMIC_PAGE_SIDE; 2]);
    let local = rgba([UI_LOCAL_FONT_PAGE_SIDE; 2]);
    let mut dynamic = (0..MAX_UI_DYNAMIC_PAGES)
        .map(|offset| match offset {
            UI_LOCAL_FONT_PAGE_OFFSET => local.clone(),
            _ => small.clone(),
        })
        .collect::<Vec<_>>();
    dynamic.extend(std::iter::repeat_n(
        rgba([UI_ART_PAGE_SIDE; 2]),
        MAX_UI_ART_PAGES,
    ));
    dynamic.extend(std::iter::repeat_n(small, MAX_UI_NATIVE_SHEET_PAGES));
    let dynamic_bytes = dynamic
        .iter()
        .map(|page| page.pixels().len())
        .sum::<usize>();
    let mut bytes = MAX_UI_FIXED_TEXTURE_BYTES - dynamic_bytes - 4;
    let full_bytes = MAX_UI_TEXTURE_SIDE as usize * MAX_UI_TEXTURE_SIDE as usize * 4;
    let full = rgba([MAX_UI_TEXTURE_SIDE; 2]);
    let mut pages = Vec::new();
    while bytes >= full_bytes {
        pages.push(full.clone());
        bytes -= full_bytes;
    }
    let row_bytes = MAX_UI_TEXTURE_SIDE as usize * 4;
    if bytes >= row_bytes {
        pages.push(rgba([MAX_UI_TEXTURE_SIDE, (bytes / row_bytes) as u32]));
        bytes %= row_bytes;
    }
    if bytes != 0 {
        pages.push(rgba([(bytes / 4) as u32, 1]));
    }
    let dynamic_start = pages.len();
    pages.extend(dynamic);
    UiTextureCatalog::new(pages, dynamic_start).unwrap()
}

#[test]
fn native_slot_growth_cannot_displace_admitted_static_artwork() {
    let base = near_fixed_limit();
    let mut pages = base.pages()[base.dynamic_start()..].to_vec();
    pages[UI_PLAYER_SKIN_PAGE_OFFSET] = rgba([render_api::MAX_STANDARD_SKIN_SIDE; 2]);
    let model = rgba([UI_MODEL_ATLAS_SIDE; 2]);
    for page in &mut pages
        [UI_MODEL_ATLAS_PAGE_OFFSET..UI_MODEL_ATLAS_PAGE_OFFSET + MAX_UI_MODEL_ATLAS_PAGES]
    {
        *page = model.clone();
    }
    pages[UI_SESSION_ICON_PAGE_OFFSET] = rgba([MAX_UI_TEXTURE_SIDE; 2]);
    let glyph = rgba([UI_SESSION_GLYPH_PAGE_SIDE; 2]);
    for page in &mut pages
        [UI_SESSION_GLYPH_PAGE_OFFSET..UI_SESSION_GLYPH_PAGE_OFFSET + UI_SESSION_GLYPH_PAGES]
    {
        *page = glyph.clone();
    }
    let resized = base
        .replace_dynamic(pages)
        .expect("native slots have independent growth capacity");
    assert_eq!(resized.fixed_budget_bytes(), base.fixed_budget_bytes());
    assert_eq!(resized.static_identity(), base.static_identity());
    assert!(std::ptr::eq(
        resized.pages()[0].pixels(),
        base.pages()[0].pixels()
    ));
    assert!(resized.plan().bytes() <= MAX_UI_TEXTURE_BYTES);
}

#[test]
fn optional_static_artwork_cannot_consume_native_slot_capacity() {
    let base = near_fixed_limit();
    let mut dynamic = base.pages()[base.dynamic_start()..].to_vec();
    dynamic[UI_PLAYER_SKIN_PAGE_OFFSET] = rgba([render_api::CLASSIC_SKIN_SIDE as u32; 2]);
    let small_skin = base.replace_dynamic(dynamic).unwrap();
    assert!(small_skin.plan().bytes() < base.plan().bytes());
    assert_eq!(small_skin.fixed_budget_bytes(), base.fixed_budget_bytes());
    let mut pages = small_skin.pages().to_vec();
    pages.insert(base.dynamic_start(), rgba([2, 1]));
    assert!(matches!(
        UiTextureCatalog::new(pages, base.dynamic_start() + 1),
        Err(UiRenderRejectReason::TextureByteLimitExceeded { actual, limit })
            if actual == MAX_UI_FIXED_TEXTURE_BYTES + 4 && limit == MAX_UI_FIXED_TEXTURE_BYTES
    ));
}

#[test]
fn original_ui_sheets_preserve_static_artwork_and_bound_aggregate_growth() {
    let base = near_fixed_limit();
    let mut pages = base.pages()[base.dynamic_start()..].to_vec();
    pages[UI_NATIVE_SHEET_PAGE_OFFSET] = rgba([MAX_UI_TEXTURE_SIDE; 2]);
    pages[UI_NATIVE_SHEET_PAGE_OFFSET + 1] = rgba([MAX_UI_TEXTURE_SIDE, MAX_UI_TEXTURE_SIDE / 2]);
    let grown = base
        .replace_dynamic(pages.clone())
        .expect("original sheets have reserved capacity");
    assert_eq!(grown.fixed_budget_bytes(), base.fixed_budget_bytes());
    assert_eq!(grown.static_identity(), base.static_identity());
    assert!(std::ptr::eq(
        grown.pages()[0].pixels(),
        base.pages()[0].pixels()
    ));
    assert!(grown.plan().bytes() <= MAX_UI_TEXTURE_BYTES);
    pages[UI_NATIVE_SHEET_PAGE_OFFSET + 2] = pages[UI_NATIVE_SHEET_PAGE_OFFSET].clone();
    assert!(matches!(
        base.replace_dynamic(pages),
        Err(UiRenderRejectReason::TextureByteLimitExceeded {
            limit: MAX_UI_NATIVE_SHEET_BYTES,
            ..
        })
    ));
}
