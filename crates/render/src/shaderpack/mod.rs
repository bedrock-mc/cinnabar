//! Experimental "Shader pack" render mode: runs a Java OptiFine/Iris shader pack, translated
//! by the `shaderpack` crate, on Cinnabar's world. Compiled with the `shaderpacks` feature.
//!
//! The block atlas (`atlas`) gives packs the single 2D `gtexture`/`normals`/`specular` atlas
//! Java builds, laid out from Cinnabar's texture-array pages.

#[cfg(feature = "shaderpacks")]
pub(crate) mod atlas;
