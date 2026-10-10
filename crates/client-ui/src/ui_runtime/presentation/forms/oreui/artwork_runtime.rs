use std::sync::Arc;

use render_model::{UiRenderTextureArray, UiTexturePage};

use super::super::super::UiPresentationRuntime;
use super::{Look, Originals};
use crate::ui_runtime::oreui_assets::OreUiImages;

impl UiPresentationRuntime {
    /// Registers shipped artwork as immutable static texture pages.
    pub fn enable_oreui_originals(&mut self, images: OreUiImages) -> Result<(), String> {
        if self
            .form_presentation
            .oreui_originals
            .as_ref()
            .is_some_and(|old| {
                old.images.pages.len() == images.pages.len()
                    && old
                        .images
                        .pages
                        .iter()
                        .zip(&images.pages)
                        .all(|(old, new)| {
                            old.dimensions == new.dimensions
                                && Arc::ptr_eq(&old.pixels, &new.pixels)
                        })
                    && old.images.sprites == images.sprites
                    && old.images.animations == images.animations
                    && old.images.loading_frames == images.loading_frames
            })
        {
            return Ok(());
        }
        let originals = images
            .pages
            .iter()
            .map(|page| {
                UiTexturePage::owned(page.dimensions, page.pixels.clone())
                    .map_err(|error| format!("{error:?}"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let dynamic_start = self.textures.dynamic_start();
        let start = self
            .form_presentation
            .oreui_originals
            .as_ref()
            .map_or(dynamic_start, |old| usize::from(old.page));
        let first = u16::try_from(start).map_err(|_| "texture page overflow".to_owned())?;
        let count = originals.len();
        let mut pages = self.textures.pages()[..start].to_vec();
        pages.extend(originals);
        pages.extend_from_slice(&self.textures.pages()[dynamic_start..]);
        let textures = UiRenderTextureArray::with_source_identity(
            pages,
            start + count,
            self.textures.static_identity(),
        )
        .map_err(|error| format!("{error:?}"))?;
        self.textures = Arc::new(textures);
        if let Some(engine) = self.form_presentation.engine.as_mut() {
            engine.textures.server_page = (self.textures.dynamic_start()
                + super::super::super::dynamic_textures::SERVER_UI_PAGE)
                as u16;
        }
        self.preview_dirty = true;
        self.menu_artwork_dirty = true;
        self.rebuild_dynamic_textures();
        self.form_presentation.oreui_look = Look::Originals;
        let masks = images
            .sprites
            .iter()
            .filter_map(|(key, sprite)| {
                key.strip_prefix("@mask/")
                    .map(|key| (key.to_owned(), *sprite))
            })
            .collect();
        self.form_presentation.oreui_originals = Some(Arc::new(Originals {
            page: first,
            images: images.clone(),
            masks,
            sprites: images.sprites,
            loading_frames: images.loading_frames,
            animations: images.animations,
        }));
        Ok(())
    }
}
