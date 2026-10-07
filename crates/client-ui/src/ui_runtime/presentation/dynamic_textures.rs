//! Composition of the bounded dynamic UI texture pages.

use std::sync::Arc;

use render_model::UiTexturePage;

use super::{IconRef, UiPresentationRuntime, item_viewmodel, menu_artwork, player_preview};

/// Dynamic page offset holding the session's server item icons.
pub(super) const SESSION_ICON_PAGE: usize = render_model::UI_SESSION_ICON_PAGE_OFFSET;
/// Dynamic pages after the general ten, holding the session's glyph-sheet atlas.
pub(super) const GLYPH_PAGES: usize = 8;
pub(super) const FIRST_GLYPH_PAGE: usize = SESSION_ICON_PAGE + 1;
/// Dynamic page offset of the server resource-pack UI textures, after the glyphs.
pub(super) const SERVER_UI_PAGE: usize = FIRST_GLYPH_PAGE + GLYPH_PAGES;
/// Dynamic pages reserved for server resource-pack UI textures.
pub(super) const SERVER_UI_PAGES: usize =
    render_model::UI_LOCAL_FONT_PAGE_OFFSET - SERVER_UI_PAGE - MODAL_UI_PAGES - MOD_UI_PAGES;
/// Dynamic page offset of a client part's modal screen textures, after the server pack's.
pub(super) const MODAL_UI_PAGE: usize = SERVER_UI_PAGE + SERVER_UI_PAGES;
/// Dynamic pages reserved for the modal's bundle textures, apart from the server pack's.
pub(super) const MODAL_UI_PAGES: usize = 4;
/// Dynamic page offset of a player mod's screen textures, after the modal's.
pub(super) const MOD_UI_PAGE: usize = MODAL_UI_PAGE + MODAL_UI_PAGES;
/// Dynamic pages reserved for the player mods' package textures, before the local font page:
/// the screen owner's first, then the HUD owner's.
pub(super) const MOD_UI_PAGES: usize = MOD_SCREEN_UI_PAGES + MOD_HUD_UI_PAGES;
pub(super) const MOD_SCREEN_UI_PAGES: usize = 2;
pub(super) const MOD_HUD_UI_PAGES: usize = 2;
/// Dynamic page offset of the HUD owner's textures, after the screen owner's.
pub(super) const MOD_HUD_UI_PAGE: usize = MOD_UI_PAGE + MOD_SCREEN_UI_PAGES;

pub(super) fn observe_session(runtime: &mut UiPresentationRuntime, session: u64) {
    let changed = runtime
        .texture_session
        .is_some_and(|previous| previous != session);
    runtime.texture_session = Some(session);
    if !changed {
        return;
    }
    runtime.player_preview_source_hash = None;
    runtime.player_preview_pose = None;
    runtime.player_preview_pixels = None;
    runtime.player_preview_drawn = None;
    runtime.gui_models.skin = None;
    runtime.held_viewmodel_source = None;
    runtime.offhand_viewmodel_source = None;
    runtime.menu_artwork_set = Default::default();
    runtime.menu_artwork_dirty = true;
    runtime.preview_dirty = true;
    rebuild(runtime);
}

/// Rebuilds dynamic pages from immutable base assets so refreshed launcher
/// artwork cannot accumulate stale layers or discard the HUD carriers.
pub(super) fn rebuild(runtime: &mut UiPresentationRuntime) {
    let width = render_model::UI_DYNAMIC_PAGE_SIDE;
    let height = render_model::UI_DYNAMIC_PAGE_SIDE;
    let layer_bytes = (width * height * 4) as usize;
    let mut rgba8 = if runtime.preview_dirty {
        vec![0; layer_bytes]
    } else {
        Vec::new()
    };
    let first_dynamic = runtime.textures.dynamic_start();
    let mut player_preview_page = runtime.player_preview_page;
    let mut player_preview_icon = runtime.player_preview_icon;
    let mut left_hand_icon = runtime.left_hand_icon;
    let mut right_hand_icon = runtime.right_hand_icon;
    let mut held_viewmodel_icon = runtime.held_viewmodel_icon;
    let mut offhand_viewmodel_icon = runtime.offhand_viewmodel_icon;

    if runtime.preview_dirty {
        player_preview_page = None;
        player_preview_icon = None;
        left_hand_icon = None;
        right_hand_icon = None;
        held_viewmodel_icon = None;
        offhand_viewmodel_icon = None;
    }

    let preview_fits = runtime.preview_dirty
        && runtime.player_preview_pixels.is_some()
        && width >= player_preview::PREVIEW_WIDTH
        && height >= player_preview::PREVIEW_HEIGHT
        && width >= player_preview::HAND_WIDTH.saturating_mul(2)
        && height >= player_preview::PREVIEW_HEIGHT.saturating_add(player_preview::HAND_HEIGHT);
    let viewmodel_fits = width >= item_viewmodel::MAIN_ORIGIN[0] + item_viewmodel::SIDE
        && height >= item_viewmodel::OFFHAND_ORIGIN[1] + item_viewmodel::SIDE;
    if preview_fits {
        let page = first_dynamic as u16;
        let layer_start = 0;
        let texture_width = width as usize;
        let copy_raster = |target: &mut [u8],
                           raster: &[u8],
                           origin: [u32; 2],
                           raster_width: u32,
                           raster_height: u32| {
            let raster_width = raster_width as usize;
            let raster_height = raster_height as usize;
            for row in 0..raster_height {
                let source_start = row * raster_width * 4;
                let target_start = layer_start
                    + ((origin[1] as usize + row) * texture_width + origin[0] as usize) * 4;
                target[target_start..target_start + raster_width * 4]
                    .copy_from_slice(&raster[source_start..source_start + raster_width * 4]);
            }
        };
        if let Some(rasters) = runtime.player_preview_pixels.as_ref() {
            for row in 0..player_preview::PREVIEW_HEIGHT as usize {
                let source_start = row * player_preview::PREVIEW_WIDTH as usize * 4;
                let target_start = layer_start + row * texture_width * 4;
                let target_end = target_start + player_preview::PREVIEW_WIDTH as usize * 4;
                rgba8[target_start..target_end].copy_from_slice(
                    &rasters.preview
                        [source_start..source_start + player_preview::PREVIEW_WIDTH as usize * 4],
                );
            }
            copy_raster(
                &mut rgba8,
                &rasters.left_hand,
                [0, player_preview::PREVIEW_HEIGHT],
                player_preview::HAND_WIDTH,
                player_preview::HAND_HEIGHT,
            );
            copy_raster(
                &mut rgba8,
                &rasters.right_hand,
                [player_preview::HAND_WIDTH, player_preview::PREVIEW_HEIGHT],
                player_preview::HAND_WIDTH,
                player_preview::HAND_HEIGHT,
            );
        }
        if viewmodel_fits {
            if let Some(main) = runtime
                .held_viewmodel_source
                .and_then(|icon| item_viewmodel::render(&runtime.textures, icon, false))
            {
                copy_raster(
                    &mut rgba8,
                    &main,
                    item_viewmodel::MAIN_ORIGIN,
                    item_viewmodel::SIDE,
                    item_viewmodel::SIDE,
                );
                held_viewmodel_icon =
                    Some(item_viewmodel::icon_at(page, item_viewmodel::MAIN_ORIGIN));
            }
            if let Some(offhand) = runtime
                .offhand_viewmodel_source
                .and_then(|icon| item_viewmodel::render(&runtime.textures, icon, true))
            {
                copy_raster(
                    &mut rgba8,
                    &offhand,
                    item_viewmodel::OFFHAND_ORIGIN,
                    item_viewmodel::SIDE,
                    item_viewmodel::SIDE,
                );
                offhand_viewmodel_icon = Some(item_viewmodel::icon_at(
                    page,
                    item_viewmodel::OFFHAND_ORIGIN,
                ));
            }
        }
        player_preview_page = Some(page);
        player_preview_icon = Some(IconRef {
            page,
            uv: [
                0,
                0,
                player_preview::PREVIEW_WIDTH as u16,
                player_preview::PREVIEW_HEIGHT as u16,
            ],
            glint: false,
        });
        left_hand_icon = Some(IconRef {
            page,
            uv: [
                0,
                player_preview::PREVIEW_HEIGHT as u16,
                player_preview::HAND_WIDTH as u16,
                player_preview::PREVIEW_HEIGHT as u16 + player_preview::HAND_HEIGHT as u16,
            ],
            glint: false,
        });
        right_hand_icon = Some(IconRef {
            page,
            uv: [
                player_preview::HAND_WIDTH as u16,
                player_preview::PREVIEW_HEIGHT as u16,
                player_preview::HAND_WIDTH.saturating_mul(2) as u16,
                player_preview::PREVIEW_HEIGHT as u16 + player_preview::HAND_HEIGHT as u16,
            ],
            glint: false,
        });
    }

    let preview = if runtime.preview_dirty {
        let Ok(page) = UiTexturePage::owned([width, height], rgba8.into()) else {
            return;
        };
        page
    } else {
        runtime.textures.pages()[first_dynamic].clone()
    };
    let mut dynamic = vec![preview];
    let art_start = first_dynamic + render_model::MAX_UI_DYNAMIC_PAGES;
    let fresh = runtime.menu_artwork_loader.pending();
    let menu_changed = fresh.is_some();
    let menu_refs = (menu_changed || runtime.menu_artwork_dirty).then(|| {
        let relative = fresh.map_or(&runtime.menu_artwork_loader.relative, |packed| &packed.refs);
        menu_artwork::rebase(relative, art_start as u16)
    });
    let previous = runtime.textures.pages();
    // Original skin and model source texels use their own dimensions. Neither page contains an
    // already-projected miniature: geometry is rasterized at the destination's physical size.
    for offset in 1..SESSION_ICON_PAGE {
        let page = if offset == super::gui_models::SKIN_PAGE {
            runtime.gui_models.skin.as_ref()
        } else if offset >= super::gui_models::MODEL_PAGE {
            runtime
                .gui_models
                .pages
                .get(offset - super::gui_models::MODEL_PAGE)
        } else {
            None
        };
        dynamic.push(
            page.cloned()
                .unwrap_or_else(|| runtime.blank_dynamic_page.clone()),
        );
    }
    dynamic.push(
        runtime
            .session_icons
            .page
            .clone()
            .unwrap_or_else(|| runtime.blank_dynamic_page.clone()),
    );
    let glyph_pages = &runtime.session_glyphs.pages;
    dynamic.extend((0..GLYPH_PAGES).map(|offset| {
        glyph_pages
            .get(offset)
            .cloned()
            .unwrap_or_else(|| runtime.blank_dynamic_page.clone())
    }));
    let server_pages = runtime.server_ui_pages();
    dynamic.extend((0..SERVER_UI_PAGES).map(|offset| {
        server_pages
            .get(offset)
            .cloned()
            .unwrap_or_else(|| runtime.blank_dynamic_page.clone())
    }));
    let modal_pages = runtime.experience_modal_pages();
    dynamic.extend((0..MODAL_UI_PAGES).map(|offset| {
        modal_pages
            .get(offset)
            .cloned()
            .unwrap_or_else(|| runtime.blank_dynamic_page.clone())
    }));
    for (pages, count) in [
        (runtime.mod_screen_pages(), MOD_SCREEN_UI_PAGES),
        (runtime.mod_hud_pages(), MOD_HUD_UI_PAGES),
    ] {
        dynamic.extend((0..count).map(|offset| {
            pages
                .get(offset)
                .cloned()
                .unwrap_or_else(|| runtime.blank_dynamic_page.clone())
        }));
    }
    dynamic.push(
        runtime
            .mod_panel_font
            .as_ref()
            .map_or_else(super::mod_panel_font::blank_page, |font| font.page.clone()),
    );
    dynamic.extend(super::font_fallback::pages(runtime));
    // Unused art pages keep their old pixels; nothing references them.
    let art_pages = previous
        .len()
        .saturating_sub(art_start)
        .min(render_model::MAX_UI_ART_PAGES);
    for offset in 0..art_pages {
        let page = match fresh.and_then(|packed| packed.pages.get(offset)) {
            Some(page) => page.clone(),
            _ => previous[art_start + offset].clone(),
        };
        dynamic.push(page);
    }
    // Equal per-page identities preserve old immutable payload ownership.
    for (offset, page) in dynamic.iter_mut().enumerate() {
        let old = &previous[first_dynamic + offset];
        if old.identity() == page.identity() {
            *page = old.clone();
        }
    }
    match runtime.textures.replace_dynamic(dynamic) {
        Ok(textures) => {
            runtime.textures = Arc::new(textures);
            runtime.player_preview_page = player_preview_page;
            runtime.player_preview_icon = player_preview_icon;
            runtime.left_hand_icon = left_hand_icon;
            runtime.right_hand_icon = right_hand_icon;
            runtime.held_viewmodel_icon = held_viewmodel_icon;
            runtime.offhand_viewmodel_icon = offhand_viewmodel_icon;
            runtime.preview_dirty = false;
            if let Some(refs) = menu_refs {
                runtime.menu_artwork.refs = refs;
                runtime.menu_artwork_dirty = false;
                runtime.refresh_full_res_art();
            }
            if menu_changed {
                runtime.menu_artwork_loader.take();
            }
        }
        Err(reason) => warn_rebuild_failed(&reason),
    }
}

/// Warns on the first failure and then each power-of-two repeat.
fn warn_rebuild_failed(reason: &render_model::UiRenderRejectReason) {
    static FAILURES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let count = FAILURES.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    if count.is_power_of_two() {
        bevy::log::warn!(count, ?reason, "dynamic UI texture pages were not replaced");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejected_rebuild_keeps_pending_artwork_and_preview_for_retry() {
        let mut runtime = UiPresentationRuntime::new(crate::test_support::fixture_font()).unwrap();
        let side = render_model::UI_ART_PAGE_SIDE;
        let pixels = vec![91; (side * side * 4) as usize];
        runtime.menu_artwork_loader.set_pending_fixture(
            vec![UiTexturePage::owned([side; 2], pixels.into()).unwrap()],
            std::collections::HashMap::from([(
                menu_artwork::TITLE_KEY.to_owned(),
                IconRef {
                    page: 0,
                    uv: [0, 0, 8, 4],
                    glint: false,
                },
            )]),
        );
        let previous = runtime.textures.clone();
        let old_refs = runtime.menu_artwork.refs.clone();
        runtime.preview_dirty = true;
        runtime.menu_artwork_dirty = true;
        runtime.gui_models.pages = vec![UiTexturePage::owned([1, 1], vec![0; 4].into()).unwrap()];
        rebuild(&mut runtime);
        assert!(Arc::ptr_eq(&runtime.textures, &previous));
        assert!(runtime.preview_dirty, "rejected pixels must be retried");
        assert!(runtime.menu_artwork_dirty);
        assert_eq!(runtime.menu_artwork.refs, old_refs);
        assert!(
            runtime.menu_artwork_loader.poll(),
            "rejected artwork stays pending"
        );
        runtime.gui_models.pages.clear();
        rebuild(&mut runtime);
        assert!(!runtime.preview_dirty);
        assert!(!runtime.menu_artwork_dirty);
        assert!(!runtime.menu_artwork_loader.poll());
        let icon = runtime.menu_artwork_icon(menu_artwork::TITLE_KEY).unwrap();
        assert_eq!(
            runtime.textures.pages()[usize::from(icon.page)].pixels()[0],
            91
        );
    }
}
