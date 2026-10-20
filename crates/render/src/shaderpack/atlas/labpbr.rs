//! Converts Cinnabar's authored normal and material layers back to LabPBR 1.3 texels.
//!
//! The pack compiler stores normals as RG unit tangent XY, B ambient occlusion and A height,
//! and materials as R F0/metal (LabPBR) or metalness (MER), G emission, B perceptual roughness
//! and A porosity/subsurface (LabPBR) or subsurface (MERS). The inverse below reproduces the
//! LabPBR source texels exactly, except that a source emission of 0 returns as 255 (both mean
//! no emission). MER sources have no exact LabPBR form: metalness becomes a metal (255) or
//! dielectric F0 at the shaders' 0.04, and subsurface moves into LabPBR's 65..=255 range.

use assets::texture::{
    PBR_REF_HEIGHT, PBR_REF_LABPBR, PBR_REF_MATERIAL, PBR_REF_NORMAL, PBR_REF_SUBSURFACE,
};

/// Flat normal, full ambient occlusion and the surface height.
pub(crate) const NEUTRAL_NORMAL: [u8; 4] = [128, 128, 255, 255];
/// Rough dielectric without F0, porosity or emission.
pub(crate) const NEUTRAL_SPECULAR: [u8; 4] = [0; 4];
/// Dielectric reflectance Cinnabar's material response assumes for MER layers.
const MER_DIELECTRIC_F0: f32 = 0.04;
/// LabPBR's "metal using albedo as F0" code; dielectric F0 values stay below 230.
const LABPBR_ALBEDO_METAL: u8 = 255;
/// LabPBR blue values from this up encode subsurface scattering.
const LABPBR_SUBSURFACE_START: u8 = 65;
/// LabPBR emission alpha that marks a texel as not emissive.
const LABPBR_NO_EMISSION: u8 = 255;

/// LabPBR normal texel for an authored normal-page texel and its authored reference flags.
pub(crate) fn labpbr_normal(texel: [u8; 4], authored_ref: u32) -> [u8; 4] {
    if authored_ref & (PBR_REF_NORMAL | PBR_REF_HEIGHT) == 0 {
        return NEUTRAL_NORMAL;
    }
    let [x, y, occlusion, height] = texel;
    let height = if authored_ref & PBR_REF_HEIGHT != 0 {
        height
    } else {
        u8::MAX
    };
    [x, y, occlusion, height]
}

/// LabPBR specular texel for an authored material-page texel and its authored reference flags.
pub(crate) fn labpbr_specular(texel: [u8; 4], authored_ref: u32) -> [u8; 4] {
    if authored_ref & PBR_REF_MATERIAL == 0 {
        return NEUTRAL_SPECULAR;
    }
    let [red, emission, roughness, auxiliary] = texel;
    let emission = if emission == 0 {
        LABPBR_NO_EMISSION
    } else {
        (f32::from(emission) * f32::from(LABPBR_NO_EMISSION - 1) / 255.0).round() as u8
    };
    let smoothness = u8::MAX - roughness;
    if authored_ref & PBR_REF_LABPBR != 0 {
        return [smoothness, red, auxiliary, emission];
    }
    let reflectance = if red >= 128 {
        LABPBR_ALBEDO_METAL
    } else {
        (MER_DIELECTRIC_F0 * 255.0).round() as u8
    };
    let subsurface = if authored_ref & PBR_REF_SUBSURFACE != 0 && auxiliary > 0 {
        let span = f32::from(u8::MAX - LABPBR_SUBSURFACE_START);
        LABPBR_SUBSURFACE_START + (f32::from(auxiliary) * span / 255.0).round() as u8
    } else {
        0
    };
    [smoothness, reflectance, subsurface, emission]
}
