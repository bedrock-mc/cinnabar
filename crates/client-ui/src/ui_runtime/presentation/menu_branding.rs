//! Prepared local menu branding shares the existing asynchronous artwork atlas and budgets.

use super::{
    UiPresentationRuntime,
    menu_artwork::{self, Artwork},
};
use image::{ImageFormat, ImageReader};
use sha2::{Digest, Sha256};
use std::{io::Cursor, sync::Arc};
use ui::IconRef;
use ui_engine::textures::{MOD_MENU_TITLE_KEY, TITLE_KEY};

/// A local component's checked menu heading and original PNG decoded off-frame.
pub struct PreparedMenuBranding {
    pub(super) logo: Option<Arc<Artwork>>,
    pub(super) title: Option<Arc<str>>,
    source_hash: Option<[u8; 32]>,
}

impl PreparedMenuBranding {
    /// Bounds PNG source dimensions before the existing alpha-correct artwork decoder runs.
    /// Source bytes stay unchanged; the atlas applies its normal display-resolution policy.
    pub fn prepare(
        png: Option<&[u8]>,
        title: Option<&str>,
        max_dimensions: [u32; 2],
        max_pixels: u64,
    ) -> Result<Self, String> {
        let logo = png
            .map(|bytes| -> Result<Arc<Artwork>, String> {
                if image::guess_format(bytes).ok() != Some(ImageFormat::Png) {
                    return Err("menu logo must be PNG".into());
                }
                let (width, height) =
                    ImageReader::with_format(Cursor::new(bytes), ImageFormat::Png)
                        .into_dimensions()
                        .map_err(|error| error.to_string())?;
                if width == 0
                    || height == 0
                    || width > max_dimensions[0]
                    || height > max_dimensions[1]
                    || u64::from(width) * u64::from(height) > max_pixels
                {
                    return Err("menu logo dimensions exceed the source limit".into());
                }
                let (pixels, width, height) =
                    menu_artwork::decode_bytes(bytes, ui_engine::pages::WHOLE_ART_PAGE)
                        .ok_or_else(|| "menu logo decoding exceeds the artwork limit".to_owned())?;
                Ok(Arc::new(Artwork {
                    width,
                    height,
                    pixels,
                }))
            })
            .transpose()?;
        if title.is_some_and(|title| title.trim().is_empty() || title.chars().any(char::is_control))
        {
            return Err("menu title must be a nonempty line".into());
        }
        Ok(Self {
            logo,
            title: title.map(Arc::from),
            source_hash: png.map(|bytes| Sha256::digest(bytes).into()),
        })
    }

    /// Identical source bytes and text preserve the accepted atlas and menu geometry.
    fn same(&self, other: &Self) -> bool {
        self.source_hash == other.source_hash && self.title == other.title
    }
}

impl UiPresentationRuntime {
    /// Changes only the selected local mod's branding; `None` immediately restores base lookup.
    /// PNG decoding and font/UI preparation must finish on the admission worker before this call.
    pub fn set_mod_menu_branding(&mut self, branding: Option<Arc<PreparedMenuBranding>>) {
        let same = match (&self.mod_menu_branding, &branding) {
            (None, None) => true,
            (Some(old), Some(new)) => old.same(new),
            _ => false,
        };
        if same {
            return;
        }
        self.mod_menu_branding = branding;
        // A superseded logo never draws while the next artwork atlas is prepared.
        self.menu_artwork.refs.remove(MOD_MENU_TITLE_KEY);
        self.retained_menu = None;
        self.sync_artwork_set(self.menu_artwork_set.clone());
    }

    /// The active prepared logo only after its latest immutable artwork pages were admitted.
    pub(super) fn mod_menu_logo(&self) -> Option<IconRef> {
        self.mod_menu_branding.as_ref()?.logo.as_ref()?;
        self.menu_artwork.refs.get(MOD_MENU_TITLE_KEY).copied()
    }

    /// Publishes text with its logo, rather than showing a new heading beside stale pixels.
    pub(super) fn mod_menu_title(&self) -> Option<&str> {
        let branding = self.mod_menu_branding.as_ref()?;
        if branding.logo.is_some() && self.mod_menu_logo().is_none() {
            return None;
        }
        branding.title.as_deref()
    }

    /// Main-menu branding falls back to the shipped original art while a new atlas is pending.
    pub(super) fn home_title_icon(&self) -> Option<IconRef> {
        self.mod_menu_logo()
            .or_else(|| self.menu_artwork.refs.get(TITLE_KEY).copied())
    }
}

#[cfg(test)]
mod tests;
