//! Borrowed texture-layer admission. The source carrier keeps its pixel ownership.

use assets::{RuntimeAssets, TextureRef};

#[derive(Debug, PartialEq, Eq)]
pub enum TextureError {
    MissingPage,
    MissingLayer,
    InvalidPixels,
    UnsupportedSize,
}

pub struct TextureLayer<'a> {
    pub(crate) size: u32,
    pub(crate) pixels: &'a [u8],
}

impl<'a> TextureLayer<'a> {
    /// Extracts only the requested base-mip layer; no whole-atlas copy is made.
    /// Animated layers, material flags and mip filtering require separate integration.
    pub fn from_assets(
        assets: &'a RuntimeAssets,
        reference: TextureRef,
    ) -> Result<Self, TextureError> {
        let page = assets
            .texture_pages()
            .get(reference.page() as usize)
            .ok_or(TextureError::MissingPage)?;
        let mip = page
            .texture
            .mips
            .first()
            .ok_or(TextureError::InvalidPixels)?;
        if reference.layer() >= page.texture.layers {
            return Err(TextureError::MissingLayer);
        }
        let bytes = (mip.size as usize)
            .checked_mul(mip.size as usize)
            .and_then(|n| n.checked_mul(4))
            .ok_or(TextureError::InvalidPixels)?;
        let start = bytes
            .checked_mul(reference.layer() as usize)
            .ok_or(TextureError::InvalidPixels)?;
        let end = start
            .checked_add(bytes)
            .ok_or(TextureError::InvalidPixels)?;
        Self::rgba(
            mip.size,
            mip.rgba8
                .get(start..end)
                .ok_or(TextureError::InvalidPixels)?,
        )
    }

    /// Current native path supports square power-of-two RGBA layers, at least 8px.
    pub fn rgba(size: u32, pixels: &'a [u8]) -> Result<Self, TextureError> {
        if size < 8 || !size.is_power_of_two() {
            return Err(TextureError::UnsupportedSize);
        }
        if (size as usize)
            .checked_mul(size as usize)
            .and_then(|n| n.checked_mul(4))
            != Some(pixels.len())
        {
            return Err(TextureError::InvalidPixels);
        }
        Ok(Self { size, pixels })
    }

    pub fn bytes(&self) -> usize {
        self.pixels.len()
    }

    pub fn size(&self) -> u32 {
        self.size
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn borrowed_layer_rejects_missing_and_truncated_pixels() {
        let assets = RuntimeAssets::diagnostic();
        let layer = TextureLayer::from_assets(&assets, TextureRef::DIAGNOSTIC).unwrap();
        assert_eq!(
            layer.pixels.as_ptr(),
            assets.texture_array().mips[0].rgba8.as_ptr()
        );
        assert_eq!(layer.bytes(), 16 * 16 * 4);
        assert!(matches!(
            TextureLayer::from_assets(&assets, TextureRef::new(0, 1).unwrap()),
            Err(TextureError::MissingLayer)
        ));
        assert!(matches!(
            TextureLayer::rgba(16, &[0; 4]),
            Err(TextureError::InvalidPixels)
        ));
        assert!(matches!(
            TextureLayer::rgba(15, &[0; 4]),
            Err(TextureError::UnsupportedSize)
        ));
    }
}
