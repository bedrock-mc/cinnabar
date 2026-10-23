//! The menus' title image: the pack's `common_art.title_image` when it defines one.

use std::collections::HashMap;

use crate::ui_runtime::presentation::{UiPresentationRuntime, menu_artwork::SERVER_ART_PREFIX};
use ui::IconRef;
use ui_engine::{
    engine::FormEngine,
    textures::{TITLE_KEY, Textures},
};

/// Where the active catalog's title image draws from; rebuilt with each installed catalog.
pub(super) struct TitleSource {
    texture: String,
    artwork: String,
}

impl TitleSource {
    pub(super) fn new(engine: &FormEngine) -> Self {
        let resolved =
            json_ui::resolve(engine.catalog(), "common_art.title_image", engine.context());
        let path = resolved
            .control
            .as_ref()
            .and_then(|control| control.properties.get("texture"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or(TITLE_KEY);
        let texture = ui_engine::textures::texture_key(path).to_owned();
        let artwork = format!("{SERVER_ART_PREFIX}{texture}");
        Self { texture, artwork }
    }

    /// Native menus use the same server-first title lookup as JSON screens.
    pub(super) fn icon(
        &self,
        engine: &FormEngine,
        images: &HashMap<String, IconRef>,
    ) -> Option<IconRef> {
        let mut atlas = engine.textures.lock();
        let key = self.texture.as_str();
        if !atlas.has_image(key) {
            return images.get(TITLE_KEY).copied();
        }
        if let Some(icon) = images.get(&self.artwork) {
            return Some(*icon);
        }
        atlas.require_current([key]);
        let textures = Textures {
            assets: engine.assets(),
            set: &engine.textures,
            atlas: &atlas,
            images: None,
        };
        let (page, [x, y, width, height]) = textures.sprite(key)?;
        Some(IconRef {
            page,
            uv: [x as u16, y as u16, (x + width) as u16, (y + height) as u16],
            glint: false,
        })
    }
}

impl UiPresentationRuntime {
    /// The installed catalog's title image; `None` without the JSON-UI engine.
    pub(super) fn title_icon(&self, images: &HashMap<String, IconRef>) -> Option<IconRef> {
        self.screens
            .menu_title
            .as_ref()?
            .icon(self.engine.as_deref()?, images)
    }

    /// Follows the catalog the engine just installed.
    pub(super) fn refresh_menu_title(&mut self) {
        self.screens.menu_title = self.engine.as_deref().map(TitleSource::new);
    }
}

#[cfg(test)]
mod tests {
    use {super::*, ui::IconRef};

    #[test]
    fn title_uses_the_pack_defined_image_and_restores_the_shipped_brand() {
        let mut presentation = crate::test_support::mini_engine_presentation();
        let key = "textures/ui/custom_server_brand";
        let definition = serde_json::to_vec(&serde_json::json!({
            "namespace":"common_art",
            "title_image":{"type":"image", "texture":format!("{key}.png")}
        }))
        .unwrap();
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(8, 4, image::Rgba([19, 53, 79, 255]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        presentation.set_server_ui_pack(&super::super::super::ServerUiPack {
            ui_layers: vec![vec![
                (
                    "ui/_ui_defs.json".into(),
                    br#"{"ui_defs":["ui/server_brand.json"]}"#.to_vec(),
                ),
                ("ui/server_brand.json".into(), definition),
            ]],
            textures: vec![(format!("{key}.png"), png)],
            ..Default::default()
        });
        let shipped = IconRef {
            page: 999,
            uv: [0, 0, 30, 10],
            glint: false,
        };
        let artwork = HashMap::from([(TITLE_KEY.to_owned(), shipped)]);
        let icon = presentation
            .title_icon(&artwork)
            .expect("small pack logo is immediately available");
        assert_ne!(icon, shipped);
        assert_eq!((icon.uv[2] - icon.uv[0], icon.uv[3] - icon.uv[1]), (8, 4));
        let engine = presentation.engine.as_deref().unwrap();
        assert!(engine.textures.lock().placement(key).is_some());
        presentation.set_server_ui_pack(&super::super::super::ServerUiPack::default());
        assert_eq!(presentation.title_icon(&artwork), Some(shipped));
    }

    #[test]
    fn oversized_title_remains_visible_to_icon_consumers_and_restores_on_reload() {
        let mut presentation = crate::test_support::mini_engine_presentation();
        let key = "textures/ui/original_large_brand";
        let shipped = IconRef {
            page: 999,
            uv: [0, 0, 30, 10],
            glint: false,
        };
        let artwork = HashMap::from([(TITLE_KEY.to_owned(), shipped)]);
        for size in [[1992, 669], [8192, 8]] {
            let definition = serde_json::to_vec(&serde_json::json!({
                "namespace":"common_art",
                "title_image":{"type":"image", "texture":key}
            }))
            .unwrap();
            let mut png = Vec::new();
            image::RgbaImage::from_pixel(size[0], size[1], image::Rgba([19, 53, 79, 255]))
                .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
                .unwrap();
            presentation.set_server_ui_pack(&super::super::super::ServerUiPack {
                ui_layers: vec![vec![
                    (
                        "ui/_ui_defs.json".into(),
                        br#"{"ui_defs":["ui/original_brand.json"]}"#.to_vec(),
                    ),
                    ("ui/original_brand.json".into(), definition),
                ]],
                textures: vec![(format!("{key}.png"), png)],
                ..Default::default()
            });
            test_time::eventually("oversized title available to icon consumers", || {
                presentation.title_icon(&artwork).is_some()
            });
            let icon = presentation.title_icon(&artwork).unwrap();
            assert_ne!(icon, shipped);
            presentation.sync_server_ui_pages();
            let page = &presentation.textures.pages()[usize::from(icon.page)];
            let offset = (usize::from(icon.uv[1]) * page.dimensions()[0] as usize
                + usize::from(icon.uv[0]))
                * 4;
            assert_eq!(&page.pixels()[offset..offset + 4], &[19, 53, 79, 255]);
            presentation.set_server_ui_pack(&super::super::super::ServerUiPack::default());
            assert_eq!(presentation.title_icon(&artwork), Some(shipped));
        }
    }
}
