//! Required item glint decoding shared by native and memory-carrier startup.

use std::{io::Cursor, sync::Arc};

use super::{RuntimeUiAssets, UiAtlasPage};
use crate::{AssetError, entity::ITEM_GLINT_TEXTURE_IDENTIFIER, texture::policy::MAX_TEXTURE_SIDE};

impl RuntimeUiAssets {
    /// Decodes the required PNG with the same size and memory bounds as UI texture pages.
    pub fn item_glint_raster(&self) -> Result<UiAtlasPage, AssetError> {
        let path = format!("{ITEM_GLINT_TEXTURE_IDENTIFIER}.png");
        let invalid = || {
            super::invalid(format!(
                "missing or invalid required item glint raster {path}"
            ))
        };
        let bytes = self.ui_file(&path).ok_or_else(invalid)?;
        let mut decoder = png::Decoder::new(Cursor::new(bytes));
        decoder.set_limits(png::Limits {
            bytes: super::MAX_UI_CARRIER_BYTES,
        });
        decoder.set_transformations(png::Transformations::EXPAND);
        let mut reader = decoder.read_info().map_err(|_| invalid())?;
        let (width, height) = (reader.info().width, reader.info().height);
        if width == 0 || height == 0 || width > MAX_TEXTURE_SIDE || height > MAX_TEXTURE_SIDE {
            return Err(invalid());
        }
        let length = reader.output_buffer_size().ok_or_else(invalid)?;
        if length > super::MAX_UI_CARRIER_BYTES {
            return Err(invalid());
        }
        let mut decoded = vec![0; length];
        let frame = reader.next_frame(&mut decoded).map_err(|_| invalid())?;
        if frame.width != width || frame.height != height {
            return Err(invalid());
        }
        decoded.truncate(frame.buffer_size());
        if frame.bit_depth == png::BitDepth::Sixteen {
            let scale = u32::from(u16::MAX) / u32::from(u8::MAX);
            for index in 0..decoded.len() / 2 {
                let value = u16::from_be_bytes([decoded[index * 2], decoded[index * 2 + 1]]);
                decoded[index] = ((u32::from(value) + scale / 2) / scale) as u8;
            }
            decoded.truncate(decoded.len() / 2);
        }
        if frame.color_type == png::ColorType::Rgba {
            return Ok(UiAtlasPage {
                width,
                height,
                rgba8: decoded.into(),
            });
        }
        let channels = frame.color_type.samples();
        let mut rgba8 = Vec::with_capacity(width as usize * height as usize * 4);
        for pixel in decoded.chunks_exact(channels) {
            let rgba = match frame.color_type {
                png::ColorType::Grayscale => [pixel[0], pixel[0], pixel[0], 255],
                png::ColorType::GrayscaleAlpha => [pixel[0], pixel[0], pixel[0], pixel[1]],
                png::ColorType::Rgb => [pixel[0], pixel[1], pixel[2], 255],
                png::ColorType::Rgba => [pixel[0], pixel[1], pixel[2], pixel[3]],
                png::ColorType::Indexed => return Err(invalid()),
            };
            rgba8.extend_from_slice(&rgba);
        }
        Ok(UiAtlasPage {
            width,
            height,
            rgba8: Arc::from(rgba8),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Encodes original pixels and embeds them in a valid UI carrier.
    fn carrier(
        width: u32,
        color: png::ColorType,
        depth: png::BitDepth,
        pixels: &[u8],
    ) -> RuntimeUiAssets {
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, width, 1);
        encoder.set_color(color);
        encoder.set_depth(depth);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(pixels)
            .unwrap();
        let blob = crate::encode_ui_catalog(
            [1; 32],
            &[],
            &[],
            &[],
            &[crate::UiFile {
                path: format!("{ITEM_GLINT_TEXTURE_IDENTIFIER}.png").into(),
                bytes: bytes.into(),
            }],
        )
        .unwrap();
        RuntimeUiAssets::decode(&blob).unwrap()
    }

    #[test]
    fn glint_preserves_rgba_and_expands_rgb_and_grayscale() {
        for (color, pixels, expected) in [
            (
                png::ColorType::Rgba,
                vec![30, 90, 150, 12],
                vec![30, 90, 150, 12],
            ),
            (
                png::ColorType::Rgb,
                vec![30, 90, 150],
                vec![30, 90, 150, 255],
            ),
            (png::ColorType::Grayscale, vec![30], vec![30, 30, 30, 255]),
            (
                png::ColorType::GrayscaleAlpha,
                vec![30, 12],
                vec![30, 30, 30, 12],
            ),
        ] {
            let raster = carrier(1, color, png::BitDepth::Eight, &pixels)
                .item_glint_raster()
                .unwrap();
            assert_eq!([raster.width, raster.height], [1, 1]);
            assert_eq!(&*raster.rgba8, expected);
        }
    }

    #[test]
    fn glint_rounds_sixteen_bit_channels_to_the_native_eight_bit_range() {
        let raster = carrier(
            1,
            png::ColorType::Rgba,
            png::BitDepth::Sixteen,
            &[0, 0, 0, 129, 128, 0, 255, 255],
        )
        .item_glint_raster()
        .unwrap();
        assert_eq!(&*raster.rgba8, &[0, 1, 128, 255]);
    }

    #[test]
    fn glint_rejects_rasters_larger_than_the_shared_texture_limit() {
        let width = MAX_TEXTURE_SIDE + 1;
        assert!(
            carrier(
                width,
                png::ColorType::Grayscale,
                png::BitDepth::Eight,
                &vec![30; width as usize]
            )
            .item_glint_raster()
            .is_err()
        );
    }
}
