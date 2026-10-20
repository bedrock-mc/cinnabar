//! Experimental "Shader pack" render mode: runs a Java OptiFine/Iris shader pack, translated
//! by the `shaderpack` crate, on Cinnabar's world. Compiled with the `shaderpacks` feature.
//!
//! The block atlas (`atlas`) gives packs the single 2D `gtexture`/`normals`/`specular` atlas
//! Java builds, laid out from Cinnabar's texture-array pages; terrain comes from the geometry
//! adapter (`geometry`). A pack's frame is planned as Iris runs it (`plan`: passes, buffers and
//! flips), its programs read Iris's standard uniforms (`uniforms`) and become raw wgpu
//! pipelines from their reflection (`pipelines`).
//!
//! Conventions: pack buffers are stored bottom-up and gamma-encoded as in OpenGL Minecraft
//! (the translator flips clip-space Y and remaps clip depth), uniforms use OpenGL matrices,
//! and each texture is read with the sampler class Iris gives its name.

#[cfg(feature = "shaderpacks")]
pub(crate) mod atlas;
#[cfg(feature = "shaderpacks")]
pub(crate) mod geometry;
#[cfg(feature = "shaderpacks")]
#[expect(dead_code, reason = "consumed by the shader-pack frame host")]
mod pipelines;
#[cfg(feature = "shaderpacks")]
#[expect(dead_code, reason = "consumed by the shader-pack frame host")]
mod plan;
#[cfg(feature = "shaderpacks")]
#[expect(dead_code, reason = "consumed by the shader-pack frame host")]
mod uniform_audit;
#[cfg(feature = "shaderpacks")]
#[expect(dead_code, reason = "consumed by the shader-pack frame host")]
mod uniforms;
