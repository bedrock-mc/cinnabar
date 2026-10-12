//! Immutable base and optional-pack item glint artwork.

use std::{io::Cursor, sync::Arc};

use render_model::UiTexturePage;

#[derive(Default)]
pub(super) struct ItemGlintTextures {
    pub(super) base: Option<Arc<UiTexturePage>>,
    pub(super) overlay: Option<Arc<UiTexturePage>>,
}

impl super::UiPresentationRuntime {
    /// Borrows the active pack's item glint, falling back to the startup carrier.
    pub fn item_glint_texture(&self) -> Option<&Arc<UiTexturePage>> {
        self.item_glint
            .overlay
            .as_ref()
            .or(self.item_glint.base.as_ref())
    }
}

/// Decodes carrier artwork once on the startup worker or during explicit UI preparation.
pub(in crate::ui_runtime) fn carrier_texture(
    assets: &assets::RuntimeUiAssets,
) -> Option<Arc<UiTexturePage>> {
    let path = format!("{}.png", assets::ITEM_GLINT_TEXTURE_PATH);
    let bytes = assets.ui_file(&path)?;
    let mut reader = image::ImageReader::with_format(Cursor::new(bytes), image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(render_model::MAX_UI_TEXTURE_SIDE);
    limits.max_image_height = Some(render_model::MAX_UI_TEXTURE_SIDE);
    limits.max_alloc = Some(assets::MAX_UI_CARRIER_BYTES as u64);
    reader.limits(limits);
    let image = reader.decode().ok()?.into_rgba8();
    UiTexturePage::owned([image.width(), image.height()], image.into_raw().into())
        .ok()
        .map(Arc::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_carrier_decodes_the_retained_item_glint_pixels() {
        let mut png = Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(3, 2, image::Rgba([30, 90, 150, 255]))
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let bytes = assets::encode_ui_catalog(
            [1; 32],
            &[],
            &[],
            &[],
            &[assets::UiFile {
                path: format!("{}.png", assets::ITEM_GLINT_TEXTURE_PATH).into(),
                bytes: png.into_inner().into(),
            }],
        )
        .unwrap();
        let carrier = assets::RuntimeUiAssets::decode(&bytes).unwrap();
        let texture = carrier_texture(&carrier).unwrap();
        assert_eq!(texture.dimensions(), [3, 2]);
        assert_eq!(texture.pixels(), [30, 90, 150, 255].repeat(6));
    }

    #[test]
    fn pack_glint_install_and_removal_restore_the_same_base_pixels() {
        let mut presentation =
            super::super::UiPresentationRuntime::new(ui_engine::test_support::fixture_font())
                .unwrap();
        let base = Arc::new(UiTexturePage::owned([1, 1], Arc::from([20, 40, 60, 255])).unwrap());
        let upper = Arc::new(
            UiTexturePage::owned([1, 2], Arc::from([200, 0, 0, 255, 0, 200, 0, 255])).unwrap(),
        );
        presentation.item_glint.base = Some(base.clone());
        presentation.set_server_ui_pack(&super::super::ServerUiPack {
            item_glint: Some(upper.clone()),
            ..Default::default()
        });
        assert!(Arc::ptr_eq(
            presentation.item_glint_texture().unwrap(),
            &upper
        ));
        assert_eq!(
            presentation.item_glint_texture().unwrap().dimensions(),
            [1, 2]
        );
        presentation.set_server_ui_pack(&super::super::ServerUiPack::default());
        assert!(Arc::ptr_eq(
            presentation.item_glint_texture().unwrap(),
            &base
        ));
        assert_eq!(
            presentation.item_glint_texture().unwrap().pixels(),
            [20, 40, 60, 255]
        );
    }
}
