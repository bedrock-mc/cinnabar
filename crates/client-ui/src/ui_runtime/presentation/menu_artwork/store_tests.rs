//! The Marketplace's offer art packs into the artwork atlas.

use std::{collections::HashMap, io::Cursor, sync::Arc};

use super::*;

// Store thumbnails decoded at 512 packed about six to the two art pages; the rest drew white.
#[test]
fn a_full_store_page_packs_every_card_thumbnail() {
    let dir = std::env::temp_dir().join(format!("cinnabar-store-art-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut jpeg = Vec::new();
    image::RgbImage::from_pixel(800, 450, image::Rgb([30, 140, 90]))
        .write_to(&mut Cursor::new(&mut jpeg), image::ImageFormat::Jpeg)
        .unwrap();
    let mut images = HashMap::new();
    let offers: Vec<_> = (0..launcher::store::snapshot::MAX_VISIBLE_IMAGES)
        .map(|index| {
            let path = dir.join(format!("{index}.jpg"));
            std::fs::write(&path, &jpeg).unwrap();
            let url = format!("https://cdn.example.test/{index}.jpg");
            images.insert(url.clone(), path.to_string_lossy().into_owned());
            protocol::store_control::StoreOffer {
                id: index.to_string(),
                title: index.to_string(),
                creator: None,
                content_type: None,
                thumbnail_url: Some(url),
                store_id: None,
                prices: vec![],
                rating: None,
                tags: vec![],
                owned: false,
            }
        })
        .collect();
    let row = |role: &'static str, offers: &[protocol::store_control::StoreOffer]| {
        launcher::store::DisplayRow {
            id: None,
            title: String::new(),
            role,
            offers: offers.to_vec(),
            continuation: None,
        }
    };
    // A plain page, and pages whose hero feature tile, decoded larger, leads or follows other rows.
    let layouts = [
        vec![row("StoreRow", &offers)],
        vec![
            row("HeroRow", &offers[..5]),
            row("HeroRow", &offers[5..10]),
            row("StoreRow", &offers[10..]),
        ],
        vec![
            row("StoreRow", &offers[..23]),
            row("HeroRow", &offers[23..28]),
            row("StoreRow", &offers[28..]),
        ],
    ];
    for rows in layouts {
        assert_store_page_packs(rows, images.clone());
    }
    std::fs::remove_dir_all(dir).unwrap();
}

fn assert_store_page_packs(
    rows: Vec<launcher::store::DisplayRow>,
    images: HashMap<String, String>,
) {
    let mut view = launcher::menu::MenuView::new(true, "Fixture Player".into());
    view.screen = launcher::menu::MenuScreen::Store;
    view.store = Some(Arc::new(launcher::store::StoreSnapshot {
        rows,
        images,
        ..launcher::store::StoreSnapshot::empty()
    }));
    let set = ArtworkSet {
        paths: view_paths(&view),
        ..Default::default()
    };
    assert_eq!(
        set.paths.len(),
        launcher::store::snapshot::MAX_VISIBLE_IMAGES
    );
    let mut cache = DecodeCache::default();
    cache.decode(&cache.missing(&set), &set);
    let packed = pack(&set, &cache, 0, true);
    let missing: Vec<_> = set
        .paths
        .iter()
        .filter(|(path, _)| !packed.refs.contains_key(path))
        .collect();
    assert!(
        missing.is_empty(),
        "{} of {} thumbnails left out",
        missing.len(),
        set.paths.len()
    );
}
