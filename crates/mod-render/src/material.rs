//! Procedural fragment-only drawing contract. Geometry and depth remain host-owned.
use mod_api::{MAX_MATERIAL_SHADER_BYTES, MAX_MATERIAL_SHADER_EXPRESSIONS};

/// Immutable admitted fragment module, shared until renderer references retire.
#[derive(Clone, Debug, PartialEq)]
pub struct Material {
    pub source: std::sync::Arc<str>,
    pub shader: std::sync::Arc<str>,
}

/// Fragment inputs also form the host vertex output; parameters are flat per instance.
pub const IO: &str = include_str!("material/io.wgsl");
/// Shared parameter-driven shader used by SDK presets.
pub const BUILTIN: &str = include_str!("material/builtin.wgsl");
/// Host view and real-time session phase, shared with the vertex stage.
pub const FRAME: &str =
    "struct DrawingView { clip: mat4x4<f32>, viewport: vec4<f32>, animation: vec4<f32> }
@group(0) @binding(0) var<uniform> drawing_view: DrawingView;
fn phase() -> f32 { return drawing_view.animation.x; }
";

/// Wraps an explicit session elapsed time; domain tests never read an application clock.
pub fn phase(elapsed_seconds: f64) -> f32 {
    if !elapsed_seconds.is_finite() {
        return 0.;
    }
    let phase =
        (elapsed_seconds.max(0.) / mod_api::materials::ANIMATION_WRAP_SECONDS).fract() as f32;
    phase.min(f32::from_bits(1.0f32.to_bits() - 1))
}

/// Composes a validated surface shader, retaining no guest resource declarations.
pub fn compose(source: &str) -> Result<String, String> {
    if source.len() > MAX_MATERIAL_SHADER_BYTES {
        return Err("material exceeds byte limit".into());
    }
    crate::shader::check_statement_tokens(source)?;
    let source = source.to_owned();
    std::thread::Builder::new()
        .name("mod material validation".into())
        .stack_size(crate::shader::VALIDATION_STACK_BYTES)
        .spawn(move || validate(&source))
        .map_err(|_| "material validation unavailable".to_owned())?
        .join()
        .map_err(|_| "material validation failed".to_owned())?
}

/// Builds the host output wrapper, clamping finite colour, coverage and world emission.
pub fn module(source: &str) -> String {
    format!(
        "{FRAME}\n{IO}\n{source}\n{}",
        include_str!("material/fragment.wgsl").replace(
            "MAX_EMISSION",
            &format!("{:.1}", mod_api::MAX_MATERIAL_EMISSION)
        )
    )
}

/// Validates only the fragment ABI: no guest vertex, depth, textures, globals or loops.
fn validate(source: &str) -> Result<String, String> {
    let composed = module(source);
    let module = naga::front::wgsl::parse_str(&composed)
        .map_err(|_| "material WGSL or shade signature invalid".to_owned())?;
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .map_err(|_| "material WGSL invalid".to_owned())?;
    crate::shader::check_sandbox(
        &module,
        &["drawing_view"],
        &["material_fragment"],
        "material_fragment",
        0,
        MAX_MATERIAL_SHADER_EXPRESSIONS,
    )?;
    Ok(composed)
}

#[cfg(test)]
mod tests;
