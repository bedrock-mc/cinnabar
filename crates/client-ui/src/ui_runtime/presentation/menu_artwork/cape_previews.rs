//! Cape attachments and card previews share the bounded artwork worker.

use protocol::CapeImage;

#[derive(Clone)]
pub(in super::super) struct CapeArtwork {
    pub(super) key: String,
    pub(super) cape: CapeImage,
    pub(super) thumbnail: bool,
}

impl CapeArtwork {
    pub(super) fn same(&self, other: &Self) -> bool {
        self.key == other.key
            && self.thumbnail == other.thumbnail
            && self.cape.width == other.cape.width
            && self.cape.height == other.cape.height
            && std::sync::Arc::ptr_eq(&self.cape.rgba8, &other.cape.rgba8)
    }

    pub(super) fn decode_key(&self) -> super::DecodeKey {
        use sha2::{Digest, Sha256};
        (
            self.key.clone(),
            if self.thumbnail {
                super::THUMBNAIL_SIDE
            } else {
                self.cape.width
            },
            Some(Sha256::digest(&self.cape.rgba8).into()),
        )
    }

    pub(super) fn decode(&self) -> Option<(Vec<u8>, u32, u32)> {
        if !self.cape.is_valid() {
            return None;
        }
        if self.thumbnail {
            use super::super::player_preview::{self, PREVIEW_HEIGHT, PREVIEW_WIDTH};
            player_preview::render_cape_thumbnail(&self.cape)
                .map(|pixels| (pixels, PREVIEW_WIDTH, PREVIEW_HEIGHT))
        } else {
            Some((self.cape.rgba8.to_vec(), self.cape.width, self.cape.height))
        }
    }
}

pub(crate) fn cape_thumbnail_key(id: &str) -> String {
    format!("cape-thumbnail:{id}")
}

pub(crate) fn cape_texture_key(cape: &CapeImage) -> String {
    format!(
        "cape-texture:{}x{}:{:p}",
        cape.width,
        cape.height,
        std::sync::Arc::as_ptr(&cape.rgba8)
    )
}

#[cfg(test)]
mod tests;
