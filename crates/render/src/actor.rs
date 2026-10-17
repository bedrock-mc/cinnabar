//! GPU actor buffers and material translation.
#[cfg(all(test, feature = "enhanced"))]
#[path = "actor/tests/enhanced_shadows.rs"]
mod enhanced_shadows;
pub(crate) mod gpu;
pub(crate) mod material;
