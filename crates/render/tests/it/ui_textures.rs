use std::sync::Arc;

use render_model::{
    MAX_UI_DYNAMIC_PAGES, MAX_UI_FIXED_TEXTURE_BYTES, MAX_UI_MODEL_ATLAS_PAGES,
    MAX_UI_TEXTURE_BYTES, MAX_UI_TEXTURE_SIDE, UI_DYNAMIC_PAGE_SIDE, UI_FALLBACK_FONT_PAGE_OFFSET,
    UI_FALLBACK_FONT_PAGE_SIDE, UI_LOCAL_FONT_PAGE_OFFSET, UI_LOCAL_FONT_PAGE_SIDE,
    UI_MODEL_ATLAS_PAGE_OFFSET, UI_MODEL_ATLAS_SIDE, UI_PLAYER_SKIN_PAGE_OFFSET,
    UI_SESSION_ICON_PAGE_OFFSET, UiRenderRejectReason, UiTextureCatalog, UiTexturePage,
    UiTexturePlan,
};

#[test]
fn mixed_dimensions_plan_native_bytes_before_materialization() {
    let mut dimensions = vec![[1024, 1024]];
    dimensions.extend([[2048, 2048]; 3]);
    dimensions.extend([[256, 256]; 15]);
    let plan = UiTexturePlan::new(&dimensions).unwrap();
    assert_eq!(plan.bytes(), 55 * 1024 * 1024 + 768 * 1024);
    assert_eq!(plan.buckets().len(), 3);
    assert_eq!(plan.locations().len(), dimensions.len());
    assert!(plan.validate_device(2048, 15).is_ok());
    assert!(plan.validate_device(1024, 256).is_err());
    assert!(plan.validate_device(4096, 14).is_err());
}

#[test]
fn planner_checks_entire_catalog_and_all_limits() {
    let mut dimensions = rgba_dimensions_for_bytes(MAX_UI_TEXTURE_BYTES);
    assert_eq!(
        UiTexturePlan::new(&dimensions).unwrap().bytes(),
        MAX_UI_TEXTURE_BYTES
    );
    dimensions.push([1; 2]);
    assert_eq!(
        UiTexturePlan::new(&dimensions),
        Err(UiRenderRejectReason::TextureByteLimitExceeded {
            actual: MAX_UI_TEXTURE_BYTES + 4,
            limit: MAX_UI_TEXTURE_BYTES,
        })
    );
    assert!(UiTexturePlan::new(&[[u32::MAX, u32::MAX]]).is_err());
    assert!(UiTexturePlan::new(&[[0, 256]]).is_err());
    assert!(UiTexturePlan::new(&vec![[1, 1]; 257]).is_err());
    let distinct_dimensions = (1..=render_model::MAX_UI_TEXTURE_LAYERS)
        .map(|n| [n, 1])
        .collect::<Vec<_>>();
    assert!(UiTexturePlan::new(&distinct_dimensions).is_ok());
    assert!(UiTexturePage::owned([1, 1], vec![0; 3].into()).is_err());
    assert!(UiTexturePage::owned([4097, 1], vec![0; 4].into()).is_err());
    assert!(UiTextureCatalog::new(Vec::new(), 0).is_err());
    let mut reserved = reserved_dynamic_pages();
    reserved.push(rgba_page([UI_DYNAMIC_PAGE_SIDE; 2], 0));
    assert!(UiTextureCatalog::new(reserved, 0).is_err());
    let unreserved = UiTexturePage::owned([1, 1], vec![0; 4].into()).unwrap();
    assert!(UiTextureCatalog::new(vec![unreserved], 0).is_err());
}

#[test]
fn dynamic_catalog_reuses_static_pixels_and_retires_old_snapshots() {
    let static_page = UiTexturePage::owned([16, 16], vec![255; 1024].into()).unwrap();
    let initial = UiTexturePage::owned([256, 256], vec![0; 256 * 256 * 4].into()).unwrap();
    let base = UiTextureCatalog::new(vec![static_page.clone(), initial], 1).unwrap();
    let mut current = Arc::new(base.clone());
    let mut prior_pixels = None;
    for value in 1..=100 {
        let retired = Arc::downgrade(&current);
        let pixels: Arc<[u8]> = vec![value; 256 * 256 * 4].into();
        let weak = Arc::downgrade(&pixels);
        let next = UiTexturePage::owned([256, 256], pixels).unwrap();
        current = Arc::new(base.replace_dynamic(vec![next]).unwrap());
        assert!(std::ptr::eq(
            current.pages()[0].pixels(),
            static_page.pixels()
        ));
        assert!(retired.upgrade().is_none());
        if let Some(old) = prior_pixels.take() {
            assert!(std::sync::Weak::upgrade(&old).is_none());
        }
        prior_pixels = Some(weak);
        assert_eq!(current.static_identity(), base.static_identity());
    }
    assert!(base.replace_dynamic(vec![]).is_err());
    assert!(base.replace_dynamic(vec![static_page]).is_err());
}

#[test]
fn ordered_logical_mapping_does_not_group_draw_order() {
    let plan = UiTexturePlan::new(&[[1024, 1024], [2048, 2048], [1024, 1024]]).unwrap();
    let logical_draws = [0usize, 1, 0, 2, 1];
    let physical = logical_draws.map(|logical| plan.locations()[logical]);
    assert_eq!(physical[0].bucket, physical[2].bucket);
    assert_ne!(physical[0].bucket, physical[1].bucket);
    assert_eq!(physical[3].layer, 1);
    assert_eq!(physical[4], physical[1]);
}

#[test]
fn art_pages_follow_the_small_dynamic_pages_within_their_own_cap() {
    let small = UiTexturePage::owned([256, 256], vec![0; 256 * 256 * 4].into()).unwrap();
    let side = render_model::UI_ART_PAGE_SIDE;
    let art =
        UiTexturePage::owned([side, side], vec![0; (side * side * 4) as usize].into()).unwrap();
    let mut pages = reserved_dynamic_pages();
    pages.extend(std::iter::repeat_n(
        art.clone(),
        render_model::MAX_UI_ART_PAGES,
    ));
    assert!(UiTextureCatalog::new(pages.clone(), 0).is_ok());
    pages.push(art);
    assert!(UiTextureCatalog::new(pages, 0).is_err());
    let odd = rgba_page([UI_MODEL_ATLAS_SIDE; 2], 0);
    assert!(UiTextureCatalog::new(vec![odd, small], 0).is_err());
}

fn rgba_page(dimensions: [u32; 2], value: u8) -> UiTexturePage {
    UiTexturePage::owned(
        dimensions,
        vec![value; (dimensions[0] * dimensions[1] * 4) as usize].into(),
    )
    .unwrap()
}

fn reserved_dynamic_pages() -> Vec<UiTexturePage> {
    let small = rgba_page([UI_DYNAMIC_PAGE_SIDE; 2], 0);
    let font = rgba_page([UI_LOCAL_FONT_PAGE_SIDE; 2], 0);
    let fallback = UiTexturePage::coverage(
        [UI_FALLBACK_FONT_PAGE_SIDE; 2],
        vec![0; (UI_FALLBACK_FONT_PAGE_SIDE * UI_FALLBACK_FONT_PAGE_SIDE) as usize].into(),
    )
    .unwrap();
    (0..MAX_UI_DYNAMIC_PAGES)
        .map(|offset| {
            if offset >= UI_FALLBACK_FONT_PAGE_OFFSET {
                fallback.clone()
            } else if offset == UI_LOCAL_FONT_PAGE_OFFSET {
                font.clone()
            } else {
                small.clone()
            }
        })
        .collect()
}

fn model_catalog() -> UiTextureCatalog {
    let mut pages = vec![rgba_page([16; 2], 255)];
    pages.extend(reserved_dynamic_pages());
    UiTextureCatalog::with_source_identity(pages, 1, [41; 32]).unwrap()
}

fn rgba_dimensions_for_bytes(bytes: usize) -> Vec<[u32; 2]> {
    assert_eq!(bytes % 4, 0);
    let side = MAX_UI_TEXTURE_SIDE as usize;
    let page_bytes = side * side * 4;
    let mut dimensions = vec![[MAX_UI_TEXTURE_SIDE; 2]; bytes / page_bytes];
    let remaining = bytes % page_bytes;
    let rows = remaining / (side * 4);
    if rows > 0 {
        dimensions.push([MAX_UI_TEXTURE_SIDE, rows as u32]);
    }
    let pixels = remaining % (side * 4) / 4;
    if pixels > 0 {
        dimensions.push([pixels as u32, 1]);
    }
    dimensions
}

fn nearly_full_catalog() -> UiTextureCatalog {
    let dynamic = reserved_dynamic_pages();
    let dynamic_bytes = dynamic
        .iter()
        .map(|page| page.pixels().len())
        .sum::<usize>();
    let dimensions = rgba_dimensions_for_bytes(MAX_UI_FIXED_TEXTURE_BYTES - dynamic_bytes - 4);
    let mut pages = dimensions
        .into_iter()
        .map(|size| rgba_page(size, 0))
        .collect::<Vec<_>>();
    let dynamic_start = pages.len();
    pages.extend(dynamic);
    let catalog = UiTextureCatalog::new(pages, dynamic_start).unwrap();
    assert_eq!(catalog.plan().bytes(), MAX_UI_FIXED_TEXTURE_BYTES - 4);
    catalog
}

#[test]
fn reserved_model_extents_roundtrip_without_changing_static_namespace_or_slot_count() {
    let base = model_catalog();
    let mut current = base.clone();
    for side in [
        render_api::CLASSIC_SKIN_SIDE as u32,
        render_api::CLASSIC_SKIN_SIDE as u32 * 2,
        UI_DYNAMIC_PAGE_SIDE,
        render_api::MAX_STANDARD_SKIN_SIDE,
    ] {
        let mut pages = current.pages()[current.dynamic_start()..].to_vec();
        pages[UI_PLAYER_SKIN_PAGE_OFFSET] = rgba_page([side; 2], 7);
        pages[UI_MODEL_ATLAS_PAGE_OFFSET] = rgba_page([UI_MODEL_ATLAS_SIDE; 2], 11);
        current = current.replace_dynamic(pages).unwrap();
        assert_eq!(current.pages().len(), base.pages().len());
        assert_eq!(current.static_identity(), base.static_identity());
        assert_eq!(
            current.pages()[1 + UI_PLAYER_SKIN_PAGE_OFFSET].dimensions(),
            [side; 2]
        );
        let index = 1 + UI_MODEL_ATLAS_PAGE_OFFSET;
        let location = current.plan().locations()[index];
        assert_eq!(
            current.plan().buckets()[location.bucket].dimensions,
            [UI_MODEL_ATLAS_SIDE; 2]
        );
        assert!(std::ptr::eq(
            current.pages()[0].pixels(),
            base.pages()[0].pixels()
        ));
    }
    let reset = current
        .replace_dynamic(base.pages()[base.dynamic_start()..].to_vec())
        .unwrap();
    assert_eq!(reset, base);
    assert_eq!(reset.plan(), base.plan());
}

#[test]
fn session_icon_resize_keeps_both_art_pages_and_static_identity() {
    let model = model_catalog();
    let mut pages = model.pages().to_vec();
    pages.extend(vec![
        rgba_page([render_model::UI_ART_PAGE_SIDE; 2], 11);
        render_model::MAX_UI_ART_PAGES
    ]);
    let base = UiTextureCatalog::new(pages, model.dynamic_start()).unwrap();
    let mut current = base.clone();
    for side in [
        UI_DYNAMIC_PAGE_SIDE * 2,
        render_model::UI_ART_PAGE_SIDE,
        UI_DYNAMIC_PAGE_SIDE,
    ] {
        let mut pages = current.pages()[current.dynamic_start()..].to_vec();
        pages[UI_SESSION_ICON_PAGE_OFFSET] = rgba_page([side; 2], 7);
        current = current.replace_dynamic(pages).unwrap();
        assert_eq!(current.pages().len(), base.pages().len());
        assert_eq!(current.static_identity(), base.static_identity());
        let icon = current.dynamic_start() + UI_SESSION_ICON_PAGE_OFFSET;
        assert_eq!(current.pages()[icon].dimensions(), [side; 2]);
        let location = current.plan().locations()[icon];
        assert_eq!(
            current.plan().buckets()[location.bucket].dimensions,
            [side; 2]
        );
        for art in current.dynamic_start() + MAX_UI_DYNAMIC_PAGES..current.pages().len() {
            assert!(std::ptr::eq(
                current.pages()[art].pixels(),
                base.pages()[art].pixels()
            ));
        }
    }
    let reset = current
        .replace_dynamic(base.pages()[base.dynamic_start()..].to_vec())
        .unwrap();
    assert_eq!(reset, base);
    assert_eq!(reset.plan(), base.plan());
}

#[test]
fn native_model_slots_cannot_expand_other_reservations_or_admit_malformed_extents() {
    let base = model_catalog();
    for (offset, dimensions) in [
        (0, [UI_MODEL_ATLAS_SIDE; 2]),
        (UI_SESSION_ICON_PAGE_OFFSET, [UI_DYNAMIC_PAGE_SIDE / 2; 2]),
        (UI_SESSION_ICON_PAGE_OFFSET, [UI_DYNAMIC_PAGE_SIDE * 3; 2]),
        (
            UI_SESSION_ICON_PAGE_OFFSET,
            [UI_DYNAMIC_PAGE_SIDE * 2, UI_DYNAMIC_PAGE_SIDE],
        ),
        (UI_PLAYER_SKIN_PAGE_OFFSET, [63; 2]),
        (
            UI_PLAYER_SKIN_PAGE_OFFSET,
            [render_api::MAX_STANDARD_SKIN_SIDE * 2; 2],
        ),
        (
            UI_MODEL_ATLAS_PAGE_OFFSET,
            [UI_MODEL_ATLAS_SIDE, UI_MODEL_ATLAS_SIDE / 2],
        ),
        (
            UI_MODEL_ATLAS_PAGE_OFFSET,
            [render_model::UI_ART_PAGE_SIDE; 2],
        ),
        (UI_LOCAL_FONT_PAGE_OFFSET, [UI_DYNAMIC_PAGE_SIDE; 2]),
        (
            UI_LOCAL_FONT_PAGE_OFFSET,
            [render_model::UI_ART_PAGE_SIDE; 2],
        ),
        (
            UI_LOCAL_FONT_PAGE_OFFSET,
            [UI_LOCAL_FONT_PAGE_SIDE, UI_LOCAL_FONT_PAGE_SIDE / 2],
        ),
        (UI_FALLBACK_FONT_PAGE_OFFSET, [UI_DYNAMIC_PAGE_SIDE; 2]),
        (UI_FALLBACK_FONT_PAGE_OFFSET, [UI_LOCAL_FONT_PAGE_SIDE; 2]),
        (
            MAX_UI_DYNAMIC_PAGES - 1,
            [UI_FALLBACK_FONT_PAGE_SIDE, UI_FALLBACK_FONT_PAGE_SIDE / 2],
        ),
    ] {
        let mut pages = base.pages()[base.dynamic_start()..].to_vec();
        pages[offset] = rgba_page(dimensions, 0);
        assert_eq!(
            base.replace_dynamic(pages),
            Err(UiRenderRejectReason::InvalidTextureExtent)
        );
    }
    let mut shorter = base.pages()[base.dynamic_start()..].to_vec();
    shorter.pop();
    assert!(base.replace_dynamic(shorter).is_err());
    let pixels = vec![0; (UI_MODEL_ATLAS_SIDE * UI_MODEL_ATLAS_SIDE * 4) as usize - 1];
    assert!(UiTexturePage::owned([UI_MODEL_ATLAS_SIDE; 2], pixels.into()).is_err());
    let mut invalid_font = reserved_dynamic_pages();
    invalid_font[UI_LOCAL_FONT_PAGE_OFFSET] = rgba_page([UI_DYNAMIC_PAGE_SIDE; 2], 0);
    assert!(UiTextureCatalog::new(invalid_font, 0).is_err());
    let pixels = vec![0; (UI_LOCAL_FONT_PAGE_SIDE * UI_LOCAL_FONT_PAGE_SIDE * 4) as usize - 1];
    assert!(UiTexturePage::owned([UI_LOCAL_FONT_PAGE_SIDE; 2], pixels.into()).is_err());
}

#[test]
fn model_resize_uses_reserved_capacity_beside_a_full_fixed_catalog() {
    let base = nearly_full_catalog();
    let dynamic_start = base.dynamic_start();
    let before = base.clone();
    let mut replacement = base.pages()[dynamic_start..].to_vec();
    for page in &mut replacement
        [UI_MODEL_ATLAS_PAGE_OFFSET..UI_MODEL_ATLAS_PAGE_OFFSET + MAX_UI_MODEL_ATLAS_PAGES]
    {
        *page = rgba_page([UI_MODEL_ATLAS_SIDE; 2], 0);
    }
    let resized = base.replace_dynamic(replacement).unwrap();
    assert_eq!(resized.fixed_budget_bytes(), base.fixed_budget_bytes());
    assert!(resized.plan().bytes() <= MAX_UI_TEXTURE_BYTES);
    assert_eq!(
        resized.pages()[dynamic_start + UI_MODEL_ATLAS_PAGE_OFFSET].dimensions(),
        [UI_MODEL_ATLAS_SIDE; 2]
    );
    assert_eq!(base, before);
    assert_eq!(
        base.pages()[dynamic_start + UI_MODEL_ATLAS_PAGE_OFFSET].dimensions(),
        [UI_DYNAMIC_PAGE_SIDE; 2]
    );
}

#[test]
fn session_icon_resize_uses_reserved_capacity_without_mutating_its_source() {
    let base = nearly_full_catalog();
    let dynamic_start = base.dynamic_start();
    let before = base.clone();
    let mut replacement = base.pages()[dynamic_start..].to_vec();
    replacement[UI_SESSION_ICON_PAGE_OFFSET] = rgba_page([UI_DYNAMIC_PAGE_SIDE * 2; 2], 7);
    let resized = base.replace_dynamic(replacement).unwrap();
    assert_eq!(resized.fixed_budget_bytes(), base.fixed_budget_bytes());
    assert!(resized.plan().bytes() <= MAX_UI_TEXTURE_BYTES);
    assert_eq!(
        resized.pages()[dynamic_start + UI_SESSION_ICON_PAGE_OFFSET].dimensions(),
        [UI_DYNAMIC_PAGE_SIDE * 2; 2]
    );
    assert_eq!(base, before);
    assert_eq!(
        base.pages()[dynamic_start + UI_SESSION_ICON_PAGE_OFFSET].dimensions(),
        [UI_DYNAMIC_PAGE_SIDE; 2]
    );
}
