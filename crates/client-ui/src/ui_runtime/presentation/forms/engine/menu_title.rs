use std::collections::HashMap;

use crate::ui_runtime::presentation::menu_artwork::{SERVER_ART_PREFIX, TITLE_KEY};
use {
    super::{FormEngine, Textures},
    ui::IconRef,
};

pub(super) struct TitleSource {
    texture: String,
    artwork: String,
}

impl TitleSource {
    pub(super) fn new(catalog: &json_ui::Catalog, context: &json_ui::Context) -> Self {
        let resolved = json_ui::resolve(catalog, "common_art.title_image", context);
        let path = resolved
            .control
            .as_ref()
            .and_then(|control| control.properties.get("texture"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or(TITLE_KEY);
        let texture = super::super::textures::texture_key(path).to_owned();
        let artwork = format!("{SERVER_ART_PREFIX}{texture}");
        Self { texture, artwork }
    }
}

impl FormEngine {
    /// Native menus use the same server-first title lookup as JSON screens.
    pub(in super::super) fn menu_title(
        &self,
        images: &HashMap<String, IconRef>,
    ) -> Option<IconRef> {
        let mut atlas = self.textures.lock();
        let key = self.menu_title_source.texture.as_str();
        if !atlas.has_image(key) {
            return images.get(TITLE_KEY).copied();
        }
        if let Some(icon) = images.get(&self.menu_title_source.artwork) {
            return Some(*icon);
        }
        atlas.require([key]);
        let textures = Textures {
            assets: &self.assets,
            set: &self.textures,
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
        let engine = presentation.form_presentation.engine.as_deref().unwrap();
        let icon = engine
            .menu_title(&artwork)
            .expect("small pack logo is immediately available");
        assert_ne!(icon, shipped);
        assert_eq!((icon.uv[2] - icon.uv[0], icon.uv[3] - icon.uv[1]), (8, 4));
        assert!(engine.textures.lock().placement(key).is_some());
        presentation.set_server_ui_pack(&super::super::super::ServerUiPack::default());
        assert_eq!(
            presentation
                .form_presentation
                .engine
                .as_deref()
                .unwrap()
                .menu_title(&artwork),
            Some(shipped)
        );
    }
}
