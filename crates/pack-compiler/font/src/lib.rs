//! Bitmap and outline font carrier compilation.

mod font;

pub use font::{
    CompiledFontCarrier, FontCompileError, FontCompileReport, GlyphAdvances, NATIVE_SDF_EM_PIXELS,
    NATIVE_SDF_MIN_PIXELS, OutlineFontConfig, compile_fonts, compile_native_outline_rasters,
    compile_outline_font, compile_outline_font_with_fallback,
};
