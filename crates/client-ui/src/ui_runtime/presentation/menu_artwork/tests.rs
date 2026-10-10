use super::*;

#[test]
fn server_details_art_fits_alongside_all_catalog_logos_without_home_promotions() {
    use launcher::menu::{MenuGameCard, MenuScreen, MenuServerCard, ServerDetails};
    for activities in [4, 12] {
        let mut view = launcher::menu::MenuView::new(true, "Fixture".into());
        view.screen = MenuScreen::Servers;
        view.featured = (0..13)
            .map(|index| MenuServerCard {
                name: index.to_string(),
                address: index.to_string(),
                caption: String::new(),
                image_path: format!("logo-{index}"),
                icon: None,
            })
            .collect();
        view.feeds.profile.avatar_path = "irrelevant-avatar".into();
        view.feeds.details.insert(
            "0".into(),
            ServerDetails {
                banner: "banner".into(),
                games: (0..activities)
                    .map(|index| MenuGameCard {
                        image_path: format!("activity-{index}"),
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            },
        );
        let set = ArtworkSet {
            paths: view_paths(&view),
            ..Default::default()
        };
        let mut cache = DecodeCache::default();
        for source in sources(&set) {
            let key = source.key();
            let width = key.1;
            let height = if key.0 == "banner" {
                width * 3 / 10
            } else {
                width
            };
            cache.decoded.insert(
                key,
                Arc::new(Artwork {
                    width,
                    height,
                    pixels: vec![255; width as usize * height as usize * 4],
                }),
            );
        }
        let atlas = pack(&set, &cache, 0, true);
        assert_eq!(
            atlas.refs["banner"].uv[2] - atlas.refs["banner"].uv[0],
            SERVER_BANNER_SIDE as u16
        );
        for index in 0..13 {
            assert!(atlas.refs.contains_key(&format!("logo-{index}")));
        }
        for index in 0..activities {
            assert!(
                atlas.refs.contains_key(&format!("activity-{index}")),
                "activity {index} of {activities} missing"
            );
        }
        assert!(!atlas.refs.contains_key("irrelevant-avatar"));
        assert!(atlas.pages.len() <= render_model::MAX_UI_ART_PAGES);
    }
}

#[test]
fn settings_atlas_prepares_the_actual_profile_gamerpic_without_a_head_substitution() {
    let mut view = launcher::menu::MenuView::new(true, "Player".into());
    view.screen = launcher::menu::MenuScreen::Settings;
    view.auth_state = launcher::menu::auth::AuthState::Authenticated;
    view.feeds.profile.picture_path = "profile-gamerpic.png".into();
    view.feeds.home.persona_head = "persona-head.png".into();
    let set = ArtworkSet {
        paths: view_paths(&view),
        ..Default::default()
    };
    let source = sources(&set)
        .into_iter()
        .find(|source| source.key().0 == view.feeds.profile.picture_path)
        .unwrap();
    let mut cache = DecodeCache::default();
    cache.decoded.insert(
        source.key(),
        Arc::new(Artwork {
            width: 8,
            height: 8,
            pixels: vec![255; 8 * 8 * 4],
        }),
    );
    let prepared = pack(&set, &cache, 0, true);
    let picture = prepared.refs.get(&view.feeds.profile.picture_path).unwrap();
    assert_eq!(picture.uv[2] - picture.uv[0], 8);
    assert_eq!(picture.uv[3] - picture.uv[1], 8);
    assert!(!prepared.refs.contains_key(&view.feeds.home.persona_head));
}

#[test]
fn large_artwork_does_not_crowd_out_a_prioritized_gamerpic() {
    let mut set = ArtworkSet {
        paths: vec![("gamerpic".into(), THUMBNAIL_SIDE)],
        ..Default::default()
    };
    set.paths
        .extend((0..8).map(|index| (format!("large-{index}"), MAX_ARTWORK_SIDE)));
    let mut cache = DecodeCache::default();
    for source in sources(&set) {
        let side = if source.key().0 == "gamerpic" {
            THUMBNAIL_SIDE
        } else {
            MAX_ARTWORK_SIDE
        };
        cache.decoded.insert(
            source.key(),
            Arc::new(Artwork {
                width: side,
                height: side,
                pixels: vec![90; side as usize * side as usize * 4],
            }),
        );
    }
    let packed = pack(&set, &cache, 0, true);
    assert!(
        packed.refs.contains_key("gamerpic"),
        "packed atlas must retain a prioritized gamerpic under capacity pressure"
    );
}

#[test]
fn a_full_server_catalog_does_not_crowd_out_account_pictures() {
    let mut view = launcher::menu::MenuView::new(true, "Player".into());
    view.feeds.account_active_id = Some("player".into());
    view.feeds.accounts = vec![launcher::accounts::AccountProfile {
        id: "player".into(),
        gamertag: "Player".into(),
        picture_path: Some("gamerpic.png".into()),
    }];
    view.featured = (0..MAX_ARTWORKS)
        .map(|index| launcher::menu::MenuServerCard {
            name: index.to_string(),
            address: index.to_string(),
            caption: String::new(),
            image_path: format!("server-{index}.png"),
            icon: None,
        })
        .collect();
    let set = ArtworkSet {
        paths: view_paths(&view),
        ..Default::default()
    };
    assert!(
        sources(&set)
            .iter()
            .any(|source| source.key().0 == "gamerpic.png"),
        "bounded atlas must retain the visible account picture"
    );
}

#[test]
fn account_pictures_are_queued_when_the_picker_opens() {
    let mut view = launcher::menu::MenuView::new(true, "First".into());
    view.feeds.account_active_id = Some("first".into());
    view.feeds.accounts = ["first", "second"]
        .map(|id| launcher::accounts::AccountProfile {
            id: id.into(),
            gamertag: id.into(),
            picture_path: Some(format!("{id}.png")),
        })
        .into();
    let paths = view_paths(&view);
    assert!(paths.iter().any(|(path, _)| path == "first.png"));
    assert!(!paths.iter().any(|(path, _)| path == "second.png"));
    view.dialog = Some(launcher::menu::MenuDialog::Accounts);
    assert!(
        view_paths(&view)
            .iter()
            .any(|(path, _)| path == "second.png")
    );
}

#[test]
fn profile_replacement_atlas_preserves_the_portrait_fallback() {
    let path = std::env::temp_dir().join(format!(
        "cinnabar-profile-portrait-{}-{}.png",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(&path, png(8, 8, [40, 80, 120, 255])).unwrap();
    let portrait = path.to_string_lossy().into_owned();
    let mut view = launcher::menu::MenuView::new(true, "Fixture Player".into());
    view.auth_state = launcher::menu::auth::AuthState::Authenticated;
    view.feeds.profile.loaded = true;
    view.feeds.profile.avatar_loaded = true;
    view.feeds.profile.featured_screenshot_loaded = true;
    view.feeds.home.persona_head = portrait.clone();
    let home = ArtworkSet {
        paths: view_paths(&view),
        ..Default::default()
    };
    let mut cache = DecodeCache::default();
    cache.decode(&cache.missing(&home), &home);
    assert!(pack(&home, &cache, 0, true).refs.contains_key(&portrait));
    view.screen = launcher::menu::MenuScreen::Profile;
    for tab in [
        launcher::menu::ProfileTab::Overview,
        launcher::menu::ProfileTab::Stats,
    ] {
        view.profile_tab = tab;
        // A missing gamerpic and a failed gamerpic decode both use the head.
        for gamerpic in [String::new(), format!("{portrait}.missing")] {
            view.feeds.profile.picture_path = gamerpic;
            let profile = ArtworkSet {
                paths: view_paths(&view),
                ..Default::default()
            };
            cache.decode(&cache.missing(&profile), &profile);
            let replacement = pack(&profile, &cache, 1, true);
            assert!(
                replacement.refs.contains_key(&portrait),
                "Profile {tab:?} replaced the atlas without its portrait fallback"
            );
        }
    }
    std::fs::remove_file(path).unwrap();
}

/// Encodes a solid test image without any external assets.
fn png(width: u32, height: u32, pixel: [u8; 4]) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::RgbaImage::from_pixel(width, height, image::Rgba(pixel))
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
        .unwrap();
    bytes
}

/// Every decode batch can be superseded before an atlas finishes packing.
#[test]
fn superseded_artwork_batches_keep_the_decode_cache_bounded() {
    let mut cache = DecodeCache::default();
    for revision in 0..2000_u32 {
        let color = [revision as u8, (revision >> 8) as u8, 20, 255];
        let set = ArtworkSet {
            oversized: vec![("changing".into(), png(8, 8, color).into())],
            ..Default::default()
        };
        cache.decode(&cache.missing(&set), &set);
        assert_eq!(cache.decoded.len(), 1);
        let source = sources(&set).pop().unwrap();
        assert_eq!(&cache.decoded[&source.key()].pixels[..4], &color);
    }
}

/// Churning distinct paths exercises eviction even when every batch is cancelled.
#[test]
fn cancelled_artwork_batches_evict_obsolete_paths() {
    let mut cache = DecodeCache::default();
    let bytes: Arc<[u8]> = png(8, 8, [10, 20, 30, 255]).into();
    for revision in 0..2000 {
        let set = ArtworkSet {
            oversized: vec![(format!("changing-{revision}"), Arc::clone(&bytes))],
            ..Default::default()
        };
        cache.decode(&cache.missing(&set), &set);
        assert!(cache.decoded.len() <= MAX_DECODED);
    }
}

#[test]
fn replacing_pack_bytes_invalidates_decoded_artwork() {
    let mut cache = DecodeCache::default();
    for color in [[200, 30, 40, 255], [20, 220, 30, 255]] {
        let set = ArtworkSet {
            oversized: vec![(TITLE_KEY.to_owned(), png(900, 300, color).into())],
            ..Default::default()
        };
        cache.decode(&cache.missing(&set), &set);
        let atlas = pack(&set, &cache, 0, true);
        let art = atlas.refs[&format!("{SERVER_ART_PREFIX}{TITLE_KEY}")];
        let page = &atlas.pages[usize::from(art.page)];
        let at = (u32::from(art.uv[1]) * page.dimensions()[0] + u32::from(art.uv[0])) as usize * 4;
        assert_eq!(&page.pixels()[at..at + 4], &color);
    }
}

#[test]
fn a_superseded_prepared_atlas_cannot_be_installed() {
    let mut loader = ArtworkLoader::default();
    assert!(loader.ready.is_some());
    loader.request(ArtworkSet::default());
    assert!(loader.ready.is_none());
    assert!(loader.take().is_none());
}

// A large server texture (Zeqa's 1992x669 title) keeps a whole art page of
// detail under its own key, not the 256px server-page downscale.
#[test]
fn oversized_server_textures_keep_full_resolution() {
    let bytes: std::sync::Arc<[u8]> = png(1992, 669, [200, 30, 40, 255]).into();
    let set = ArtworkSet {
        paths: Vec::new(),
        oversized: vec![(TITLE_KEY.to_owned(), bytes)],
        ..Default::default()
    };
    let mut cache = DecodeCache::default();
    cache.decode(&cache.missing(&set), &set);
    let atlas = pack(&set, &cache, 0, true);
    let art = atlas.refs[&format!("{SERVER_ART_PREFIX}{TITLE_KEY}")];
    let [u0, v0, u1, v1] = art.uv;
    assert_eq!([u1 - u0, v1 - v0], [1022, 343]);
    // Cinnabar's logo keeps the plain title key.
    assert_ne!(atlas.refs[TITLE_KEY].uv, art.uv);
}

// Artwork stays straight alpha, as the UI shader samples it.
#[test]
fn artwork_is_straight_alpha() {
    let (pixels, _, _) = decode_bytes(&png(4, 4, [200, 100, 50, 128]), 64).unwrap();
    assert_eq!(&pixels[..4], &[200, 100, 50, 128]);
    let (scaled, width, _) = decode_bytes(&png(128, 128, [200, 100, 50, 128]), 64).unwrap();
    assert_eq!(width, 64);
    assert!(
        scaled[..3]
            .iter()
            .zip([200, 100, 50])
            .all(|(a, b)| a.abs_diff(b) <= 1)
    );
}
#[test]
fn grayscale_conversion_obeys_the_decode_memory_budget() {
    let mut bytes = Vec::new();
    image::GrayImage::new(4096, 4096)
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
        .unwrap();
    assert!(decode_bytes(&bytes, 128).is_none());
}

#[test]
fn oversized_source_sets_cannot_defeat_the_decode_cache_bound() {
    let bytes: Arc<[u8]> = png(2, 2, [255; 4]).into();
    let set = ArtworkSet {
        oversized: (0..MAX_DECODED + 1)
            .map(|i| (format!("texture-{i}"), bytes.clone()))
            .collect(),
        ..Default::default()
    };
    let mut cache = DecodeCache::default();
    let missing = cache.missing(&set);
    assert!(missing.len() <= MAX_ARTWORKS);
    cache.decode(&missing, &set);
    assert!(cache.decoded.len() <= MAX_DECODED);
}

#[test]
fn repeated_artwork_paths_do_not_exclude_later_unique_sources() {
    let set = ArtworkSet {
        paths: std::iter::repeat_n(("same".into(), 128), MAX_ARTWORKS)
            .chain([("later".into(), 128)])
            .collect(),
        ..Default::default()
    };
    assert_eq!(sources(&set).len(), 2);
}
