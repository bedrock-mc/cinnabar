//! Bitmap and outline font carrier compilation.

mod font;

pub use font::{
    CompiledFontCarrier, FontCompileError, FontCompileReport, GlyphAdvances, OutlineFontConfig,
    compile_fonts, compile_outline_font, compile_outline_font_with_fallback,
};
#[cfg(not(target_arch = "wasm32"))]
pub use font::{NATIVE_SDF_EM_PIXELS, NATIVE_SDF_MIN_PIXELS, compile_native_outline_rasters};
