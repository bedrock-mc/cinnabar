//! Featured thumbnails cached off the frame loop.
use crate::{
    launcher_account::ARTWORK_BUDGET,
    remote_images::{ImageDirectory, LAUNCHER_ART},
};
use crossbeam_channel::Receiver;
use launcher::menu::view::MenuServerCard;
use std::path::PathBuf;

#[derive(Debug)]
pub struct CatalogArt(Receiver<Vec<(String, String)>>);
impl CatalogArt {
    /// Takes completed thumbnail paths without waiting for downloads.
    pub fn poll(&self) -> Option<Vec<(String, String)>> {
        self.0.try_recv().ok()
    }
}

/// Starts bounded thumbnail downloads on a worker.
pub fn start(directory: PathBuf, wanted: Vec<(String, String)>) -> Option<CatalogArt> {
    let (sender, receiver) = crossbeam_channel::bounded(1);
    std::thread::Builder::new()
        .name("catalog-art".to_owned())
        .spawn(move || {
            let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                return;
            };
            let images = ImageDirectory::new(directory, LAUNCHER_ART);
            let (addresses, urls): (Vec<String>, Vec<String>) = wanted.into_iter().unzip();
            let paths = runtime.block_on(images.fetch_all(urls, ARTWORK_BUDGET));
            let cached = addresses
                .into_iter()
                .zip(paths)
                .filter_map(|(address, path)| Some((address, path?.to_string_lossy().into_owned())))
                .collect();
            let _ = sender.send(cached);
        })
        .ok()?;
    Some(CatalogArt(receiver))
}

/// Sets each listed card's thumbnail unless it already has one.
pub fn apply(cards: &mut [MenuServerCard], paths: &[(String, String)]) {
    for card in cards.iter_mut().filter(|card| card.image_path.is_empty()) {
        if let Some((_, path)) = paths.iter().find(|(address, _)| *address == card.address) {
            card.image_path.clone_from(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_thumbnails_reach_only_their_own_empty_cards() {
        let card = |address: &str, path: &str| MenuServerCard {
            name: address.to_owned(),
            address: address.to_owned(),
            caption: String::new(),
            image_path: path.to_owned(),
            icon: None,
        };
        let mut cards = vec![card("a:1", ""), card("b:1", "/kept.png"), card("c:1", "")];
        apply(
            &mut cards,
            &[
                ("a:1".into(), "/a.img".into()),
                ("b:1".into(), "/b.img".into()),
            ],
        );
        let paths: Vec<_> = cards.iter().map(|card| card.image_path.as_str()).collect();
        assert_eq!(paths, ["/a.img", "/kept.png", ""]);
    }
}
