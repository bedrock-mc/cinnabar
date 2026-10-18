//! Immutable gallery thumbnails prepared on the artwork worker.

use super::{ArtworkSet, UiPresentationRuntime};
use launcher::dressing_room::{cape_thumbnail_key, thumbnail_key};
use launcher::menu::{MenuScreen, MenuView};

#[derive(Clone)]
pub(in super::super) struct SkinArtwork {
    pub(super) key: String,
    pub(super) skin: render_api::StandardSkin,
}

impl SkinArtwork {
    pub(super) fn same(&self, other: &Self) -> bool {
        self.key == other.key
            && self.skin.width == other.skin.width
            && self.skin.height == other.skin.height
            && self.skin.rgba8 == other.skin.rgba8
            && super::super::player_preview::cape::same(
                self.skin.cape.as_ref(),
                other.skin.cape.as_ref(),
            )
            && match (&self.skin.geometry, &other.skin.geometry) {
                (None, None) => true,
                (Some(a), Some(b)) => std::sync::Arc::ptr_eq(a, b),
                _ => false,
            }
    }

    pub(super) fn decode_key(&self) -> super::DecodeKey {
        use sha2::{Digest, Sha256};
        let mut hash = Sha256::new();
        hash.update(self.skin.rgba8.content_hash().to_le_bytes());
        hash.update([u8::from(self.skin.cape.is_some())]);
        if let Some(cape) = &self.skin.cape {
            hash.update(cape.width.to_le_bytes());
            hash.update(cape.height.to_le_bytes());
            hash.update(&cape.rgba8);
        }
        if let Some(geometry) = &self.skin.geometry {
            hash.update(geometry.resource_patch.as_bytes());
            hash.update(geometry.geometry_data.as_bytes());
        }
        (
            self.key.clone(),
            super::THUMBNAIL_SIDE,
            Some(hash.finalize().into()),
        )
    }

    pub(super) fn decode(&self) -> Option<(Vec<u8>, u32, u32)> {
        use super::super::player_preview::{self, PREVIEW_HEIGHT, PREVIEW_WIDTH};
        player_preview::render_skin_thumbnail(&self.skin)
            .map(|pixels| (pixels, PREVIEW_WIDTH, PREVIEW_HEIGHT))
    }
}

impl UiPresentationRuntime {
    /// Prepares visible skin thumbnails without rasterizing them during a frame.
    pub fn sync_menu_artwork_view(&mut self, view: &MenuView) {
        if self
            .menu_artwork_loader
            .gallery
            .as_ref()
            .is_some_and(|request| {
                request.matches(
                    view,
                    &self.menu_skin_thumbnail_indices,
                    &self.menu_cape_thumbnail_indices,
                    &self.menu_cosmetic_thumbnail_indices,
                    &self.menu_preview_model.cosmetics,
                )
            })
        {
            if self.menu_artwork_loader.poll() {
                self.rebuild_dynamic_textures();
            }
            return;
        }
        let request = super::request_cache::GalleryRequest::capture(
            view,
            &self.menu_skin_thumbnail_indices,
            &self.menu_cape_thumbnail_indices,
            &self.menu_cosmetic_thumbnail_indices,
            &self.menu_preview_model.cosmetics,
        );
        let skins = if view.screen == MenuScreen::DressingRoom {
            let mut indices: Vec<_> = view.dressing_room.selected.into_iter().collect();
            for index in &self.menu_skin_thumbnail_indices {
                if !indices.contains(index) {
                    indices.push(*index);
                }
            }
            if self.menu_skin_thumbnail_indices.is_empty() {
                for index in 0..view.dressing_room.skins.len().min(12) {
                    if !indices.contains(&index) {
                        indices.push(index);
                    }
                }
            }
            indices
                .into_iter()
                .filter_map(|index| view.dressing_room.skins.get(index))
                .map(|entry| SkinArtwork {
                    key: thumbnail_key(&entry.id),
                    skin: entry.skin.clone(),
                })
                .collect()
        } else {
            Vec::new()
        };
        let mut capes: Vec<_> = view
            .player_skin
            .as_ref()
            .and_then(|skin| skin.cape.as_ref())
            .map(|cape| super::CapeArtwork {
                key: super::cape_texture_key(cape),
                cape: cape.clone(),
                thumbnail: false,
            })
            .into_iter()
            .collect();
        if view.screen == MenuScreen::DressingRoom
            && view.dressing_room.section == launcher::dressing_room::DressingRoomSection::Capes
        {
            let mut indices: Vec<_> = view.dressing_room.selected_cape.into_iter().collect();
            for index in &self.menu_cape_thumbnail_indices {
                if !indices.contains(index) {
                    indices.push(*index);
                }
            }
            if self.menu_cape_thumbnail_indices.is_empty() {
                indices.extend(0..view.dressing_room.capes.len().min(12));
            }
            capes.extend(
                indices
                    .into_iter()
                    .filter_map(|index| view.dressing_room.capes.get(index))
                    .map(|entry| super::CapeArtwork {
                        key: cape_thumbnail_key(&entry.id),
                        cape: entry.cape.clone(),
                        thumbnail: true,
                    }),
            );
        }
        self.sync_artwork_set(ArtworkSet {
            logo: self.menu_artwork_set.logo.clone(),
            paths: super::view_paths(view),
            images: super::view_images(view, &self.menu_hit_targets),
            oversized: self.oversized_ui_textures(),
            skins,
            capes,
            cosmetics: super::cosmetic_requests::requested(
                &self.menu_preview_model.cosmetics,
                &self.menu_cosmetic_thumbnail_indices,
                view.screen == MenuScreen::DressingRoom
                    && view.dressing_room.section
                        == launcher::dressing_room::DressingRoomSection::Cosmetics,
            ),
        });
        self.menu_artwork_loader.gallery = request;
    }
}

#[cfg(test)]
mod tests {
    use super::super::{DecodeCache, Source, sources};
    use launcher::dressing_room::{DressingRoomSkin, DressingRoomView};
    use std::sync::Arc;
    use {
        super::*,
        launcher::menu::{MenuScreen, MenuView},
    };

    #[test]
    fn unchanged_gallery_artwork_sync_allocates_nothing() {
        let _ = render_model::hats::fixture::gallery();
        use launcher::dressing_room::{DressingRoomCape, DressingRoomSection};
        let mut presentation = UiPresentationRuntime::new(ui_engine::test_support::fixture_font())
            .expect("diagnostic presentation");
        let cape = render_api::CapeImage {
            width: render_api::CAPE_DIMENSIONS[0].0,
            height: render_api::CAPE_DIMENSIONS[0].1,
            rgba8: vec![
                255;
                (render_api::CAPE_DIMENSIONS[0].0 * render_api::CAPE_DIMENSIONS[0].1 * 4)
                    as usize
            ]
            .into(),
        };
        let skin = render_api::StandardSkin {
            width: 64,
            height: 64,
            rgba8: vec![255; 64 * 64 * 4].into(),
            cape: Some(cape.clone()),
            geometry: None,
        };
        let mut view = MenuView::new(true, "Fixture".into());
        view.screen = MenuScreen::DressingRoom;
        view.player_skin = Some(skin.clone());
        view.dressing_room = std::sync::Arc::new(DressingRoomView {
            skins: vec![DressingRoomSkin {
                engine_version: render_api::DEFAULT_SKIN_GEOMETRY_ENGINE_VERSION.into(),
                id: "skin".into(),
                name: "Skin".into(),
                path: String::new(),
                imported: true,
                model: launcher::dressing_room::SkinModel::Classic,
                skin,
            }]
            .into(),
            selected: Some(0),
            capes: vec![DressingRoomCape {
                id: "cape".into(),
                name: "Cape".into(),
                path: String::new(),
                imported: true,
                cape,
            }]
            .into(),
            selected_cape: Some(0),
            ..Default::default()
        });
        presentation.menu_skin_thumbnail_indices = vec![0];
        presentation.menu_cape_thumbnail_indices = vec![0];
        for section in [DressingRoomSection::Skins, DressingRoomSection::Capes] {
            std::sync::Arc::make_mut(&mut view.dressing_room).section = section;
            presentation.sync_menu_artwork_view(&view);
            presentation.finish_menu_artwork();
            let (_, allocations) = test_alloc::count(|| {
                for _ in 0..8 {
                    presentation.sync_menu_artwork_view(&view);
                }
            });
            assert_eq!(allocations, 0);
        }
    }

    #[test]
    fn cosmetic_artwork_requests_only_equipped_items_outside_the_gallery() {
        let catalog = render_model::hats::fixture::gallery();
        let equipped = *catalog.last().unwrap();
        let mut presentation = UiPresentationRuntime::new(ui_engine::test_support::fixture_font())
            .expect("diagnostic presentation");
        presentation.set_preview_cosmetics(&[equipped]);
        let view = MenuView::new(true, "Fixture".into());
        presentation.sync_menu_artwork_view(&view);
        let requested: Vec<_> = sources(&presentation.menu_artwork_set)
            .into_iter()
            .filter_map(|source| match source {
                Source::Item(item, thumbnail) => Some((item, thumbnail)),
                _ => None,
            })
            .collect();
        assert_eq!(requested, vec![(equipped, false)]);
        presentation.sync_menu_artwork(Vec::new());
        assert!(
            sources(&presentation.menu_artwork_set)
                .iter()
                .any(|source| source.key().0 == equipped.texture_key())
        );
    }

    #[test]
    fn cosmetic_gallery_requests_follow_visible_items_without_displacing_skin_art() {
        let catalog = render_model::hats::fixture::gallery();
        let equipped = *catalog.last().unwrap();
        let mut presentation =
            UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap();
        presentation.set_preview_cosmetics(&[equipped]);
        let mut view = MenuView::new(true, "Fixture".into());
        view.screen = MenuScreen::DressingRoom;
        let wardrobe = std::sync::Arc::make_mut(&mut view.dressing_room);
        wardrobe.section = launcher::dressing_room::DressingRoomSection::Cosmetics;
        wardrobe.skins = vec![DressingRoomSkin {
            id: "fixture-skin".into(),
            name: "Skin".into(),
            path: String::new(),
            imported: false,
            model: launcher::dressing_room::SkinModel::Classic,
            engine_version: render_api::DEFAULT_SKIN_GEOMETRY_ENGINE_VERSION.into(),
            skin: render_api::StandardSkin {
                width: 64,
                height: 64,
                rgba8: vec![255; 64 * 64 * 4].into(),
                cape: None,
                geometry: None,
            },
        }]
        .into();
        wardrobe.selected = Some(0);
        for shown in [catalog[catalog.len() - 2], equipped] {
            presentation.menu_cosmetic_thumbnail_indices = vec![
                cosmetics::Item::all()
                    .iter()
                    .position(|item| *item == shown)
                    .unwrap(),
            ];
            presentation.sync_menu_artwork_view(&view);
            let requested = sources(&presentation.menu_artwork_set);
            assert!(requested.len() < super::super::MAX_ARTWORKS);
            assert!(
                requested
                    .iter()
                    .any(|source| source.key().0 == thumbnail_key("fixture-skin"))
            );
            let items: Vec<_> = requested
                .into_iter()
                .filter_map(|source| match source {
                    Source::Item(item, thumbnail) => Some((item, thumbnail)),
                    _ => None,
                })
                .collect();
            let mut expected = vec![(equipped, false)];
            expected.push((shown, true));
            if shown != equipped {
                expected.push((shown, false));
            }
            assert_eq!(items, expected);
        }
    }

    #[test]
    fn cosmetic_gallery_keeps_every_visible_thumbnail_when_animation_textures_reach_the_budget() {
        let gallery = render_model::hats::fixture::gallery();
        let equipped = render_model::hats::fixture::crown();
        let all = cosmetics::Item::all();
        let visible: Vec<_> = gallery
            .iter()
            .map(|item| all.iter().position(|candidate| candidate == item).unwrap())
            .collect();
        let set = ArtworkSet {
            cosmetics: super::super::cosmetic_requests::requested(&[equipped], &visible, true),
            ..Default::default()
        };
        let admitted = sources(&set);
        assert!(admitted.len() <= super::super::MAX_ARTWORKS);
        assert!(matches!(admitted.first(), Some(Source::Item(item, false)) if *item == equipped));
        for &item in gallery {
            assert!(
                admitted
                    .iter()
                    .any(|source| matches!(source, Source::Item(shown, true) if *shown == item)),
                "missing visible thumbnail for {}",
                item.id()
            );
        }
    }

    #[test]
    fn cosmetic_geometry_revision_prepares_bounds_when_its_texture_is_cached() {
        let [original, revised] = render_model::hats::fixture::wing_revisions();
        let mut cache = DecodeCache::default();
        let mut set = ArtworkSet {
            cosmetics: super::super::cosmetic_requests::requested(&[original], &[], false),
            ..Default::default()
        };
        let missing = cache.missing(&set);
        cache.decode(&missing, &set);
        let key = Source::Item(original, false).key();
        let texture = Arc::clone(&cache.decoded[&key]);
        assert_eq!(original.texture_key(), revised.texture_key());
        assert!(render_model::hats::thumbnail::prepared_frame(revised).is_none());
        set.cosmetics = super::super::cosmetic_requests::requested(&[revised], &[], false);
        let missing = cache.missing(&set);
        assert!(render_model::hats::thumbnail::prepared_frame(revised).is_none());
        assert_eq!(missing.len(), 1);
        cache.decode(&missing, &set);
        assert!(cache.missing(&set).is_empty());
        let frame = render_model::hats::thumbnail::prepared_frame(revised)
            .expect("new rig bounds despite the texture cache hit");
        assert_ne!(
            frame.model_bounds(),
            render_model::hats::thumbnail::prepared_frame(original)
                .unwrap()
                .model_bounds()
        );
        assert!(Arc::ptr_eq(&texture, &cache.decoded[&key]));
    }

    #[test]
    fn revealed_skin_after_the_atlas_limit_gets_requested() {
        let mut presentation = UiPresentationRuntime::new(ui_engine::test_support::fixture_font())
            .expect("diagnostic presentation");
        let source = render_api::StandardSkin {
            width: 64,
            height: 64,
            rgba8: vec![255; 64 * 64 * 4].into(),
            cape: None,
            geometry: None,
        };
        let mut view = MenuView::new(true, "Fixture".into());
        view.screen = MenuScreen::DressingRoom;
        view.dressing_room = std::sync::Arc::new(DressingRoomView {
            skins: (0..100)
                .map(|index| DressingRoomSkin {
                    engine_version: render_api::DEFAULT_SKIN_GEOMETRY_ENGINE_VERSION.into(),
                    id: format!("skin-{index}"),
                    name: format!("Skin {index}"),
                    skin: source.clone(),
                    path: String::new(),
                    imported: true,
                    model: launcher::dressing_room::SkinModel::Classic,
                })
                .collect::<Vec<_>>()
                .into(),
            selected: Some(0),
            ..Default::default()
        });
        presentation.menu_skin_thumbnail_indices = vec![98, 99];
        presentation.sync_menu_artwork_view(&view);
        let keys: Vec<_> = sources(&presentation.menu_artwork_set)
            .iter()
            .map(Source::key)
            .collect();
        assert_eq!(keys.len(), 3);
        assert!(keys.iter().any(|key| key.0 == thumbnail_key("skin-99")));
        let before = presentation.menu_artwork_set.clone();
        presentation.sync_menu_artwork_view(&view);
        assert!(before.same(&presentation.menu_artwork_set));
    }

    #[test]
    fn unchanged_skin_gallery_reuses_decoded_thumbnails() {
        let source = SkinArtwork {
            key: thumbnail_key("fixture"),
            skin: render_api::StandardSkin {
                width: 64,
                height: 64,
                rgba8: vec![255; 64 * 64 * 4].into(),
                cape: None,
                geometry: None,
            },
        };
        let set = ArtworkSet {
            skins: vec![source],
            ..Default::default()
        };
        assert!(set.same(&set.clone()));
        assert!(matches!(sources(&set).first(), Some(Source::Skin(_))));
        let mut cache = DecodeCache::default();
        cache.decode(&cache.missing(&set), &set);
        assert!(cache.missing(&set).is_empty());
        let artwork = cache
            .decoded
            .values()
            .next()
            .expect("skin raster is cached");
        assert!(
            artwork
                .pixels
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[3] > 0)
        );
        let mut changed = set.clone();
        changed.skins[0].skin.rgba8 = vec![100; 64 * 64 * 4].into();
        assert!(!set.same(&changed));
        assert_eq!(cache.missing(&changed).len(), 1);
    }

    #[test]
    fn explicit_artwork_request_does_not_leave_the_gallery_stale() {
        let mut presentation = UiPresentationRuntime::new(ui_engine::test_support::fixture_font())
            .expect("diagnostic presentation");
        let mut view = MenuView::new(true, "Fixture".into());
        view.screen = MenuScreen::DressingRoom;
        view.dressing_room = std::sync::Arc::new(DressingRoomView {
            skins: vec![DressingRoomSkin {
                engine_version: render_api::DEFAULT_SKIN_GEOMETRY_ENGINE_VERSION.into(),
                id: "restored".into(),
                name: "Skin".into(),
                path: String::new(),
                imported: true,
                model: launcher::dressing_room::SkinModel::Classic,
                skin: render_api::StandardSkin {
                    width: 64,
                    height: 64,
                    rgba8: vec![255; 64 * 64 * 4].into(),
                    cape: None,
                    geometry: None,
                },
            }]
            .into(),
            selected: Some(0),
            ..Default::default()
        });
        presentation.sync_menu_artwork_view(&view);
        presentation.finish_menu_artwork();
        presentation.sync_menu_artwork(vec![("missing-fixture-art.png".into(), 64)]);
        presentation.sync_menu_artwork_view(&view);
        presentation.finish_menu_artwork();
        assert!(
            presentation
                .menu_artwork_icon(&thumbnail_key("restored"))
                .is_some()
        );
        assert!(presentation.menu_artwork_set.paths.is_empty());
    }

    #[test]
    fn replacing_a_skin_thumbnail_cape_invalidates_its_cached_artwork() {
        let skin = render_api::StandardSkin {
            width: 64,
            height: 64,
            rgba8: vec![255; 64 * 64 * 4].into(),
            cape: None,
            geometry: None,
        };
        let bare = SkinArtwork {
            key: "cape-replacement".into(),
            skin,
        };
        let mut caped = bare.clone();
        let (width, height) = render_api::CAPE_DIMENSIONS[0];
        caped.skin.cape = Some(render_api::CapeImage {
            width,
            height,
            rgba8: [17, 229, 61, 255].repeat((width * height) as usize).into(),
        });
        assert!(!bare.same(&caped));
        assert!(bare.decode_key() != caped.decode_key());
        let mut cache = DecodeCache::default();
        let old = ArtworkSet {
            skins: vec![bare],
            ..Default::default()
        };
        cache.decode(&cache.missing(&old), &old);
        let new = ArtworkSet {
            skins: vec![caped],
            ..Default::default()
        };
        assert_eq!(cache.missing(&new).len(), 1);
    }
}
