//! Vanilla trigonometry table initialization and lookup.
//! Evaluate only the addressed sample; no retained table or per-frame allocation.

const INDEX_SCALE: f32 = 10_430.378;

fn sample(index: i32) -> f32 {
    (f32::from(index as u16) / INDEX_SCALE).sin()
}

pub(crate) fn sine(radians: f32) -> f32 {
    sample((radians * INDEX_SCALE) as i32)
}

pub(crate) fn cosine(radians: f32) -> f32 {
    // Apply the quarter-turn before truncation, not to the already truncated index.
    sample((radians * INDEX_SCALE + 16_384.0) as i32)
}
