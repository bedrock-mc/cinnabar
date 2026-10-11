//! Optional cape downloads verify immutable identities without blocking skin selection.

use launcher::dressing_room::DressingRoomCape;
use {super::*, launcher::install_layout::InstallLayout};

pub(super) fn cached(layout: &InstallLayout) -> Vec<DressingRoomCape> {
    default_capes::JAVA_CAPES
        .iter()
        .filter_map(|(name, hash, digest)| {
            let path = cache_path(layout, hash);
            let bytes = png_bytes(&path).ok()?;
            let cape = validated(digest, &bytes).ok()?;
            Some(DressingRoomCape {
                id: format!("java-cape:{hash}"),
                name: (*name).to_owned(),
                path: path.to_string_lossy().into_owned(),
                cape,
                imported: false,
            })
        })
        .collect()
}

pub(crate) fn merge(layout: &InstallLayout, view: &mut DressingRoomView) {
    merge_entries(view, cached(layout));
    if view.selected_cape.is_none() {
        let saved = read_bounded(&layout.skin_selection_file(), MAX_PREFERENCES_BYTES)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Preferences>(&bytes).ok())
            .and_then(|saved| saved.selected_cape);
        view.selected_cape =
            saved.and_then(|id| view.capes.iter().position(|entry| entry.id == id));
    }
}

fn merge_entries(view: &mut DressingRoomView, defaults: Vec<DressingRoomCape>) {
    let mut capes = view.capes.to_vec();
    let initial = capes.len();
    for entry in defaults {
        if !capes.iter().any(|current| current.id == entry.id) {
            capes.push(entry);
        }
    }
    if capes.len() != initial {
        view.capes = capes.into();
    }
}

#[cfg(not(test))]
pub(crate) fn refresh(layout: &InstallLayout) {
    if default_capes::JAVA_CAPES.is_empty() {
        return;
    }
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return;
    };
    runtime.block_on(async {
        let Ok(client) = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(8))
            .build()
        else {
            return;
        };
        let mut jobs = tokio::task::JoinSet::new();
        for (_, hash, digest) in default_capes::JAVA_CAPES {
            let path = cache_path(layout, hash);
            if png_bytes(&path)
                .ok()
                .is_some_and(|bytes| validated(digest, &bytes).is_ok())
            {
                continue;
            }
            let client = client.clone();
            let hash = (*hash).to_owned();
            let digest = (*digest).to_owned();
            jobs.spawn(async move {
                if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                    return;
                }
                let Ok(response) = client
                    .get(format!("https://textures.minecraft.net/texture/{hash}"))
                    .send()
                    .await
                else {
                    return;
                };
                if !response.status().is_success()
                    || response
                        .content_length()
                        .is_some_and(|size| size > MAX_PNG_BYTES)
                {
                    return;
                }
                let mut response = response;
                let mut bytes = Vec::new();
                loop {
                    match response.chunk().await {
                        Ok(Some(chunk))
                            if bytes.len().saturating_add(chunk.len())
                                <= MAX_PNG_BYTES as usize =>
                        {
                            bytes.extend_from_slice(&chunk)
                        }
                        Ok(None) => break,
                        _ => return,
                    }
                }
                if validated(&digest, &bytes).is_err() {
                    return;
                }
                let Some(parent) = path.parent() else {
                    return;
                };
                if fs::create_dir_all(parent).is_err() {
                    return;
                }
                static NEXT_FILE: std::sync::atomic::AtomicU64 =
                    std::sync::atomic::AtomicU64::new(0);
                let nonce = NEXT_FILE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let temp = path.with_extension(format!("{}.{nonce}.tmp", std::process::id()));
                if fs::write(&temp, bytes).is_ok() {
                    let _ = fs::rename(&temp, &path);
                }
                let _ = fs::remove_file(temp);
            });
        }
        while jobs.join_next().await.is_some() {}
    });
}

fn cache_path(layout: &InstallLayout, hash: &str) -> PathBuf {
    layout
        .dressing_room_capes_dir()
        .join("runtime")
        .join(format!("{hash}.png"))
}

fn validated(hash: &str, bytes: &[u8]) -> Result<render_api::CapeImage, String> {
    if format!("{:x}", Sha256::digest(bytes)) != hash {
        return Err("The cape image does not match its texture identity.".to_owned());
    }
    capes::decode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn official_cache_rejects_changed_texels_even_when_dimensions_are_valid() {
        let size = render_api::CAPE_DIMENSIONS[0];
        let png = image::RgbaImage::from_pixel(size.0, size.1, image::Rgba([20, 50, 100, 255]));
        let mut bytes = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(png)
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        let bytes = bytes.into_inner();
        let hash = format!("{:x}", Sha256::digest(&bytes));
        assert!(validated(&hash, &bytes).unwrap().is_valid());
        assert!(validated(&"0".repeat(hash.len()), &bytes).is_err());
    }

    #[test]
    fn refreshing_defaults_preserves_imported_indices_selected_cape_and_drafts() {
        let size = render_api::CAPE_DIMENSIONS[0];
        let cape = render_api::CapeImage {
            width: size.0,
            height: size.1,
            rgba8: vec![255; (size.0 * size.1 * 4) as usize].into(),
        };
        let imported = DressingRoomCape {
            id: "imported-cape".to_owned(),
            name: "Custom".to_owned(),
            path: "private.png".to_owned(),
            cape: cape.clone(),
            imported: true,
        };
        let official = DressingRoomCape {
            id: "runtime-cape".to_owned(),
            name: "Runtime".to_owned(),
            path: "runtime.png".to_owned(),
            cape,
            imported: false,
        };
        let mut view = DressingRoomView {
            capes: vec![imported.clone()].into(),
            selected_cape: Some(0),
            ..Default::default()
        };
        view.editor = Some(launcher::dressing_room::SkinEditor {
            index: 0,
            mode: launcher::dressing_room::SkinEditorMode::Rename,
            target: launcher::dressing_room::SkinEditorTarget::Cape,
            draft: "Pending name".to_owned(),
        });
        let editor = view.editor.clone();
        merge_entries(&mut view, vec![official.clone()]);
        assert_eq!(view.capes[0], imported);
        assert_eq!(view.capes[1], official);
        assert_eq!(view.selected_cape, Some(0));
        assert_eq!(view.editor, editor);
        let retained = view.capes.clone();
        merge_entries(&mut view, vec![official]);
        assert!(Arc::ptr_eq(&view.capes, &retained));
    }
}
