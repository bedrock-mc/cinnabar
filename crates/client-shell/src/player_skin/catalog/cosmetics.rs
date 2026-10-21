//! Admits only catalog content with every asset present and hash-checked in the host cache.
use launcher::install_layout::InstallLayout;

/// Loads cached optional content without doing network work on the caller's thread.
pub(super) fn load(layout: &InstallLayout) {
    let root = launcher_host::cosmetics::cache_dir(layout);
    let Some(catalog) = launcher_host::cosmetics::cached(&root) else {
        return;
    };
    let mut admitted = Vec::new();
    for item in catalog.items {
        let Some(geometry) = launcher_host::cosmetics::read_asset(&root, &item.assets.geometry)
        else {
            continue;
        };
        let Some(texture) = launcher_host::cosmetics::read_asset(&root, &item.assets.texture)
        else {
            continue;
        };
        let animation = match &item.assets.animation {
            Some(asset) => {
                let Some(bytes) = launcher_host::cosmetics::read_asset(&root, asset) else {
                    continue;
                };
                bytes
            }
            None => Vec::new(),
        };
        if let Some(item) = render_model::hats::admit(item, &geometry, &texture, &animation) {
            admitted.push(item.id().to_owned());
        }
    }
    cosmetics::activate(&admitted);
    let mut emotes = Vec::new();
    for item in catalog.emotes {
        let Some(bytes) = launcher_host::cosmetics::read_asset(&root, &item.clip) else {
            cosmetics::emotes::reject();
            continue;
        };
        if let Some(item) = cosmetics::emotes::Item::admit(item, &bytes) {
            emotes.push(item);
        } else {
            bevy::log::warn!("malformed catalog emote skipped");
        }
    }
    cosmetics::emotes::activate(&emotes);
}

/// Refreshes catalog files on the wardrobe worker, retaining usable cached content during outages.
#[cfg(not(any(test, feature = "test-support")))]
pub(crate) fn refresh(layout: &InstallLayout) {
    let _ = launcher_host::cosmetics::refresh(&launcher_host::cosmetics::cache_dir(layout));
    load(layout);
}
