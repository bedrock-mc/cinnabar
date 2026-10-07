use std::ptr;

use assets::{FontLineMetrics, MAX_FONT_PAGE_SIDE, MAX_FONT_SOURCE_BYTES};
use freetype_sys as ft;

use super::super::super::{FontCompileError, RasterizedGlyph, invalid};

struct Library(ft::FT_Library);

impl Drop for Library {
    fn drop(&mut self) {
        // Initialization succeeded, and each face is released before its library.
        unsafe { ft::FT_Done_FreeType(self.0) };
    }
}

pub(super) struct Face<'a> {
    raw: ft::FT_Face,
    _library: Library,
    _source: &'a [u8],
    size: u32,
}

impl<'a> Face<'a> {
    pub(super) fn new(source: &'a [u8], size: u32) -> Result<Self, FontCompileError> {
        let length = ft::FT_Long::try_from(source.len())
            .map_err(|_| invalid("outline source length exceeds FreeType bounds"))?;
        let mut library = ptr::null_mut();
        // The out pointer is writable; source stays borrowed for the face's entire lifetime.
        check(unsafe { ft::FT_Init_FreeType(&mut library) })?;
        let library = Library(library);
        let mut raw = ptr::null_mut();
        check(unsafe { ft::FT_New_Memory_Face(library.0, source.as_ptr(), length, 0, &mut raw) })?;
        let face = Self {
            raw,
            _library: library,
            _source: source,
            size,
        };
        check(unsafe { ft::FT_Set_Pixel_Sizes(face.raw, 0, size) })?;
        Ok(face)
    }

    pub(super) fn has(&self, codepoint: char) -> bool {
        // The face and its selected Unicode charmap remain owned by this wrapper.
        unsafe { ft::FT_Get_Char_Index(self.raw, codepoint as ft::FT_ULong) != 0 }
    }

    pub(super) fn line_metrics(&self) -> Result<FontLineMetrics, FontCompileError> {
        // FreeType retains this face record until Drop.
        let face = unsafe { &*self.raw };
        let units = u32::from(face.units_per_EM);
        if units == 0 || face.ascender <= 0 || face.descender > 0 {
            return Err(invalid("outline face has invalid line metrics"));
        }
        let scale = |units_value: i32| {
            (f64::from(units_value) * f64::from(self.size) * 64.0 / f64::from(units)).round() as u32
        };
        Ok(FontLineMetrics {
            em_64: self.size * 64,
            ascent_64: scale(i32::from(face.ascender)),
            descent_64: scale(-i32::from(face.descender)),
        })
    }

    pub(super) fn rasterize(
        &mut self,
        codepoint: char,
    ) -> Result<RasterizedGlyph, FontCompileError> {
        // Every call completes before the mutable glyph slot is reused.
        let index = unsafe { ft::FT_Get_Char_Index(self.raw, codepoint as ft::FT_ULong) };
        check(unsafe {
            ft::FT_Load_Glyph(
                self.raw,
                index,
                ft::FT_LOAD_NO_HINTING | ft::FT_LOAD_NO_BITMAP,
            )
        })?;
        let slot = unsafe { (*self.raw).glyph };
        check(unsafe { ft::FT_Render_Glyph(slot, ft::FT_RENDER_MODE_NORMAL) })?;
        let slot = unsafe { &*slot };
        let bitmap = &slot.bitmap;
        let width = u32::try_from(bitmap.width).map_err(|_| invalid("negative glyph width"))?;
        let height = u32::try_from(bitmap.rows).map_err(|_| invalid("negative glyph height"))?;
        let pitch = bitmap.pitch.unsigned_abs();
        if width > MAX_FONT_PAGE_SIDE
            || height > MAX_FONT_PAGE_SIDE
            || u64::from(pitch) * u64::from(height) > MAX_FONT_SOURCE_BYTES
            || (width != 0
                && height != 0
                && (bitmap.buffer.is_null()
                    || pitch < width
                    || bitmap.pixel_mode as u8 != ft::FT_PIXEL_MODE_GRAY as u8
                    || bitmap.num_grays != 256))
        {
            return Err(invalid(
                "outline raster dimensions or coverage exceed bounds",
            ));
        }
        let mut alpha = Vec::with_capacity(width as usize * height as usize);
        if width != 0 {
            for row in 0..height {
                // FreeType's signed pitch identifies a valid row in its live bitmap allocation.
                let pixels = unsafe {
                    std::slice::from_raw_parts(
                        bitmap.buffer.offset(row as isize * bitmap.pitch as isize),
                        width as usize,
                    )
                };
                alpha.extend_from_slice(pixels);
            }
        }
        let advance = (slot.linearHoriAdvance as f64 / 1024.0).round();
        if advance < 0.0 || advance > i16::MAX as f64 {
            return Err(invalid("outline advance exceeds bounds"));
        }
        Ok(RasterizedGlyph {
            codepoint,
            width,
            height,
            bearing: [
                i16::try_from(slot.bitmap_left)
                    .map_err(|_| invalid("glyph bearing exceeds bounds"))?,
                i16::try_from(slot.bitmap_top)
                    .map_err(|_| invalid("glyph bearing exceeds bounds"))?
                    .checked_neg()
                    .ok_or_else(|| invalid("glyph bearing exceeds bounds"))?,
            ],
            advance_64: advance as i16,
            alpha: alpha.into_boxed_slice(),
        })
    }
}

impl Drop for Face<'_> {
    fn drop(&mut self) {
        // The owned face precedes the library field's automatic destructor.
        unsafe { ft::FT_Done_Face(self.raw) };
    }
}

fn check(error: ft::FT_Error) -> Result<(), FontCompileError> {
    if error == 0 {
        Ok(())
    } else {
        Err(invalid(format!("FreeType outline error {error}")))
    }
}
