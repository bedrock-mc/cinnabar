//! Raw custom textures from `shaders.properties`: the `<TYPE> <internalFormat> <size>
//! <pixelFormat> <pixelType>` specification and the conversion of its pixels to texel bytes.

use super::MAX_CUSTOM_TEXTURE_SIDE;
use crate::shaderpack::geometry::f32_to_half;
use crate::shaderpack::pipelines::{Kind, format_kind};
use shaderpack::{BufferFormat, TextureDimension};
use wgpu::TextureFormat;

/// A parsed raw texture specification.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct RawSpec {
    pub(super) dimension: TextureDimension,
    pub(super) internal: BufferFormat,
    pub(super) size: [u32; 3],
    channels: usize,
    bgr: bool,
    pixel: PixelType,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PixelType {
    U8,
    I8,
    U16,
    I16,
    U32,
    I32,
    F16,
    F32,
}

impl PixelType {
    fn bytes(self) -> usize {
        match self {
            PixelType::U8 | PixelType::I8 => 1,
            PixelType::U16 | PixelType::I16 | PixelType::F16 => 2,
            PixelType::U32 | PixelType::I32 | PixelType::F32 => 4,
        }
    }

    /// Component `bytes` as f64: normalized for integer pixel types unless
    /// `integer`, raw otherwise.
    fn read(self, bytes: &[u8], integer: bool) -> f64 {
        let norm = |v: f64, max: f64| if integer { v } else { (v / max).max(-1.0) };
        match self {
            PixelType::U8 => norm(f64::from(bytes[0]), 255.0),
            PixelType::I8 => norm(f64::from(bytes[0] as i8), 127.0),
            PixelType::U16 => norm(f64::from(u16::from_le_bytes([bytes[0], bytes[1]])), 65535.0),
            PixelType::I16 => norm(f64::from(i16::from_le_bytes([bytes[0], bytes[1]])), 32767.0),
            PixelType::U32 => norm(
                f64::from(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])),
                f64::from(u32::MAX),
            ),
            PixelType::I32 => norm(
                f64::from(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])),
                f64::from(i32::MAX),
            ),
            PixelType::F16 => f64::from(half_to_f32(u16::from_le_bytes([bytes[0], bytes[1]]))),
            PixelType::F32 => {
                f64::from(f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
            }
        }
    }
}

fn half_to_f32(h: u16) -> f32 {
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exponent = i32::from((h >> 10) & 0x1f);
    let mantissa = f32::from(h & 0x3ff);
    sign * match exponent {
        0 => mantissa * 2f32.powi(-24),
        31 if mantissa == 0.0 => f32::INFINITY,
        31 => f32::NAN,
        e => (1.0 + mantissa / 1024.0) * 2f32.powi(e - 15),
    }
}

impl RawSpec {
    pub(super) fn parse(spec: &[String]) -> Option<RawSpec> {
        let dimension = match spec.first()?.as_str() {
            "TEXTURE_1D" => TextureDimension::D1,
            "TEXTURE_2D" | "TEXTURE_RECTANGLE" => TextureDimension::D2,
            "TEXTURE_3D" => TextureDimension::D3,
            _ => return None,
        };
        let dims = match dimension {
            TextureDimension::D1 => 1,
            TextureDimension::D3 => 3,
            _ => 2,
        };
        let internal = BufferFormat::parse(spec.get(1)?)?;
        let mut size = [1u32; 3];
        for (i, slot) in size.iter_mut().take(dims).enumerate() {
            *slot = spec
                .get(2 + i)?
                .parse()
                .ok()
                .filter(|v| (1..=MAX_CUSTOM_TEXTURE_SIDE).contains(v))?;
        }
        let order = spec.get(2 + dims)?.trim_end_matches("_INTEGER");
        let channels = match order {
            "RED" | "R" => 1,
            "RG" => 2,
            "RGB" | "BGR" => 3,
            "RGBA" | "BGRA" => 4,
            _ => return None,
        };
        let pixel = match spec.get(3 + dims)?.as_str() {
            "UNSIGNED_BYTE" => PixelType::U8,
            "BYTE" => PixelType::I8,
            "UNSIGNED_SHORT" => PixelType::U16,
            "SHORT" => PixelType::I16,
            "UNSIGNED_INT" => PixelType::U32,
            "INT" => PixelType::I32,
            "HALF_FLOAT" => PixelType::F16,
            "FLOAT" => PixelType::F32,
            _ => return None,
        };
        Some(RawSpec {
            dimension,
            internal,
            size,
            channels,
            bgr: matches!(order, "BGR" | "BGRA"),
            pixel,
        })
    }

    /// Converts the file's pixels into `format`'s texel bytes; `None` if the data is short
    /// or the format is not one the converter writes.
    pub(super) fn convert(&self, bytes: &[u8], format: TextureFormat) -> Option<Vec<u8>> {
        let texels = self.size.iter().map(|v| *v as usize).product::<usize>();
        let stride = self.channels * self.pixel.bytes();
        if bytes.len() < texels * stride {
            return None;
        }
        let integer = format_kind(format) != Kind::Float;
        let (channels, write): (usize, fn(f64, &mut Vec<u8>)) = match format {
            TextureFormat::R8Unorm => (1, |v, o| o.push((v.clamp(0.0, 1.0) * 255.0).round() as u8)),
            TextureFormat::Rg8Unorm => {
                (2, |v, o| o.push((v.clamp(0.0, 1.0) * 255.0).round() as u8))
            }
            TextureFormat::Rgba8Unorm => {
                (4, |v, o| o.push((v.clamp(0.0, 1.0) * 255.0).round() as u8))
            }
            TextureFormat::R16Float => (1, |v, o| o.extend(f32_to_half(v as f32).to_le_bytes())),
            TextureFormat::Rg16Float => (2, |v, o| o.extend(f32_to_half(v as f32).to_le_bytes())),
            TextureFormat::Rgba16Float => (4, |v, o| o.extend(f32_to_half(v as f32).to_le_bytes())),
            TextureFormat::R32Float => (1, |v, o| o.extend((v as f32).to_le_bytes())),
            TextureFormat::Rg32Float => (2, |v, o| o.extend((v as f32).to_le_bytes())),
            TextureFormat::Rgba32Float => (4, |v, o| o.extend((v as f32).to_le_bytes())),
            TextureFormat::R32Uint => (1, |v, o| o.extend((v.max(0.0) as u32).to_le_bytes())),
            TextureFormat::R32Sint => (1, |v, o| o.extend((v as i32).to_le_bytes())),
            _ => return None,
        };
        let mut out = Vec::with_capacity(texels * channels * 4);
        for texel in bytes.chunks_exact(stride).take(texels) {
            for c in 0..channels {
                let value = if c < self.channels {
                    let source = if self.bgr && c < 3 { 2 - c } else { c };
                    let at = source * self.pixel.bytes();
                    self.pixel
                        .read(&texel[at..at + self.pixel.bytes()], integer)
                } else if c == 3 {
                    1.0
                } else {
                    0.0
                };
                write(value, &mut out);
            }
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|p| p.to_string()).collect()
    }

    #[test]
    fn raw_specs_parse_shape_format_and_pixel_layout() {
        let raw = RawSpec::parse(&spec(&[
            "TEXTURE_3D",
            "RGB16F",
            "4",
            "2",
            "3",
            "RGB",
            "HALF_FLOAT",
        ]))
        .unwrap();
        assert_eq!(raw.dimension, TextureDimension::D3);
        assert_eq!(raw.internal, BufferFormat::Rgb16f);
        assert_eq!(raw.size, [4, 2, 3]);
        assert_eq!((raw.channels, raw.pixel), (3, PixelType::F16));
        assert!(
            RawSpec::parse(&spec(&[
                "TEXTURE_2D",
                "RGBA8",
                "0",
                "4",
                "RGBA",
                "UNSIGNED_BYTE"
            ]))
            .is_none()
        );
        assert!(
            RawSpec::parse(&spec(&[
                "TEXTURE_CUBE",
                "RGBA8",
                "4",
                "4",
                "RGBA",
                "UNSIGNED_BYTE"
            ]))
            .is_none()
        );
    }

    #[test]
    fn raw_bgr_pixels_preserve_red_and_blue_channels() {
        let raw =
            RawSpec::parse(&spec(&["TEXTURE_1D", "RGBA8", "1", "BGR", "UNSIGNED_BYTE"])).unwrap();
        assert_eq!(
            raw.convert(&[0, 0, 255], TextureFormat::Rgba8Unorm)
                .unwrap(),
            [255, 0, 0, 255]
        );
    }

    #[test]
    fn raw_bgra_pixels_preserve_color_and_alpha() {
        let raw = RawSpec::parse(&spec(&[
            "TEXTURE_1D",
            "RGBA8",
            "1",
            "BGRA",
            "UNSIGNED_BYTE",
        ]))
        .unwrap();
        assert_eq!(
            raw.convert(&[0, 0, 255, 51], TextureFormat::Rgba8Unorm)
                .unwrap(),
            [255, 0, 0, 51]
        );
    }

    #[test]
    fn raw_pixels_widen_to_the_texture_format() {
        let raw =
            RawSpec::parse(&spec(&["TEXTURE_1D", "RGB16F", "2", "RGB", "HALF_FLOAT"])).unwrap();
        let mut bytes = vec![];
        for v in [0.5f32, 1.0, -2.0, 0.25, 0.0, 3.0] {
            bytes.extend(f32_to_half(v).to_le_bytes());
        }
        let out = raw.convert(&bytes, TextureFormat::Rgba16Float).unwrap();
        let halves: Vec<f32> = out
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| half_to_f32(u16::from_le_bytes(*b)))
            .collect();
        assert_eq!(halves, [0.5, 1.0, -2.0, 1.0, 0.25, 0.0, 3.0, 1.0]);
        assert!(
            raw.convert(&bytes[..4], TextureFormat::Rgba16Float)
                .is_none()
        );
        let bytes8 = RawSpec::parse(&spec(&[
            "TEXTURE_2D",
            "RGBA8",
            "1",
            "1",
            "RED",
            "UNSIGNED_BYTE",
        ]))
        .unwrap()
        .convert(&[51], TextureFormat::Rgba8Unorm)
        .unwrap();
        assert_eq!(bytes8, [51, 0, 0, 255]);
    }
    #[test]
    fn raw_32_bit_components_normalize_for_noninteger_textures() {
        for (pixel, bytes) in [
            ("UNSIGNED_INT", 0x8000_0000_u32.to_le_bytes()),
            ("INT", 0x4000_0000_i32.to_le_bytes()),
        ] {
            let raw =
                RawSpec::parse(&spec(&["TEXTURE_2D", "RGBA8", "1", "1", "RED", pixel])).unwrap();
            assert_eq!(
                raw.convert(&bytes, TextureFormat::Rgba8Unorm).unwrap(),
                [128, 0, 0, 255]
            );
        }
        assert_eq!(
            PixelType::U32.read(&u32::MAX.to_le_bytes(), true),
            f64::from(u32::MAX)
        );
        assert_eq!(
            PixelType::I32.read(&i32::MIN.to_le_bytes(), true),
            f64::from(i32::MIN)
        );
    }

    #[test]
    fn signed_normalized_components_clamp_the_minimum_to_negative_one() {
        assert_eq!(PixelType::I8.read(&i8::MIN.to_le_bytes(), false), -1.0);
        assert_eq!(PixelType::I16.read(&i16::MIN.to_le_bytes(), false), -1.0);
        assert_eq!(PixelType::I8.read(&i8::MIN.to_le_bytes(), true), -128.0);
    }
}
