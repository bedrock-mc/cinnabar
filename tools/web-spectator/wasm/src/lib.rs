//! Read-only browser terrain geometry using Cinnabar's world and greedy mesher.
//!
//! This optional extension is diagnostic, not a vanilla visual parity target.
//! It never opens a game connection or accepts gameplay input.

mod geometry;
mod materials;
mod model;
mod terrain;

#[cfg(test)]
mod tests;

/// Builds triangle vertices in world coordinates from a bounded arena snapshot.
///
/// Each vertex has nine floats: position XYZ, normal XYZ, linear color RGB.
/// The JavaScript binding returns a `Float32Array` and throws on invalid input.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub fn mesh_arena(input: &str) -> Result<Vec<f32>, String> {
    let arena = model::Arena::parse(input)?;
    terrain::mesh(&arena)
}
