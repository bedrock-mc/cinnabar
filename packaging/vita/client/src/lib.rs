//! Vita platform integration. This library is not a complete game entry point.
//!
//! The renderer consumes Cinnabar's CPU meshes, avoiding the desktop wgpu pipeline.
//! Admission counts pending uploads and resources still in use by the GPU.

pub mod controls;
pub mod memory;
#[cfg(target_os = "vita")]
pub mod native;
pub mod player;
pub mod terrain;
pub mod texture;
