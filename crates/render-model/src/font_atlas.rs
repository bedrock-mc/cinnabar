//! Bounded glyph-rectangle residency shared by GPU preparation and offline checks.

use std::collections::HashMap;

#[cfg(test)]
mod bench_tests;
mod frame;
pub use frame::{FontAtlasFrame, FontAtlasVertex};

/// A font page uses at most one MiB of R8 GPU storage.
pub const FONT_ATLAS_SIDE: u32 = assets::FONT_FALLBACK_ATLAS_SIDE;
/// Bounds residency metadata even for one-texel glyphs.
pub const MAX_FONT_ATLAS_GLYPHS: usize = 4096;
/// Glyph rectangles include one texel of their original sampling neighborhood.
pub const FONT_ATLAS_GUTTER: u16 = 1;

pub type FontRect = [u16; 4];

#[derive(Clone, Copy, Debug, Default)]
struct Shelf {
    x: u32,
    y: u32,
    height: u32,
}

impl Shelf {
    /// Reserves a rectangle and its sampling gutter without exceeding the page.
    fn reserve(&mut self, rect: FontRect, side: u32) -> Option<[u16; 2]> {
        let pad = u32::from(FONT_ATLAS_GUTTER);
        let width = u32::from(rect[2].checked_sub(rect[0])?) + 2 * pad;
        let height = u32::from(rect[3].checked_sub(rect[1])?) + 2 * pad;
        if rect[0] == rect[2] || rect[1] == rect[3] || width > side || height > side {
            return None;
        }
        if self.x + width > side {
            self.x = 0;
            self.y += self.height;
            self.height = 0;
        }
        if self.y + height > side {
            return None;
        }
        let position = [(self.x + pad) as u16, (self.y + pad) as u16];
        self.x += width;
        self.height = self.height.max(height);
        Some(position)
    }
}

/// Retains glyphs until the page fills, then repacks only the current frame's glyphs.
#[derive(Debug)]
pub struct FontAtlas {
    side: u32,
    shelf: Shelf,
    resident: HashMap<FontRect, [u16; 2]>,
    uploads: Vec<FontRect>,
}

impl Default for FontAtlas {
    fn default() -> Self {
        Self {
            side: FONT_ATLAS_SIDE,
            shelf: Shelf::default(),
            resident: HashMap::new(),
            uploads: Vec::new(),
        }
    }
}

impl FontAtlas {
    /// Creates a cache with an admitted power-of-two texture side.
    pub fn new(side: u32) -> Self {
        assert!(side.is_power_of_two() && side <= FONT_ATLAS_SIDE);
        Self {
            side,
            ..Self::default()
        }
    }

    /// Resolves every sorted, unique request before any texture write or draw can occur.
    pub fn prepare(&mut self, requested: &[FontRect]) -> bool {
        self.uploads.clear();
        if requested.len() > MAX_FONT_ATLAS_GLYPHS {
            return false;
        }
        let mut count = self.resident.len();
        let mut next = self.shelf;
        let fits = requested.iter().all(|rect| {
            self.resident.contains_key(rect) || {
                count += 1;
                count <= MAX_FONT_ATLAS_GLYPHS && next.reserve(*rect, self.side).is_some()
            }
        });
        if !fits {
            let mut empty = Shelf::default();
            if !requested
                .iter()
                .all(|rect| empty.reserve(*rect, self.side).is_some())
            {
                return false;
            }
            self.resident.clear();
            self.shelf = Shelf::default();
        }
        for &rect in requested {
            if !self.resident.contains_key(&rect) {
                let Some(position) = self.shelf.reserve(rect, self.side) else {
                    return false;
                };
                self.resident.insert(rect, position);
                self.uploads.push(rect);
            }
        }
        true
    }

    /// Returns the resident origin corresponding to the requested rectangle's top-left texel.
    pub fn origin(&self, rect: FontRect) -> Option<[u16; 2]> {
        self.resident.get(&rect).copied()
    }

    /// Rectangles whose texels must be issued before this frame is drawn.
    pub fn uploads(&self) -> &[FontRect] {
        &self.uploads
    }

    /// Forgets residency after a source-page or GPU-allocation replacement.
    pub fn clear(&mut self) {
        self.shelf = Shelf::default();
        self.resident.clear();
        self.uploads.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requested_glyphs_are_resident_in_the_first_frame_and_warm_frames_do_no_uploads() {
        let mut atlas = FontAtlas::default();
        let requested = [[0, 0, 18, 18], [20, 0, 38, 18]];
        assert!(atlas.prepare(&requested));
        assert_eq!(atlas.uploads(), requested);
        assert!(requested.iter().all(|rect| atlas.origin(*rect).is_some()));
        let positions = requested.map(|rect| atlas.origin(rect));
        assert!(atlas.prepare(&requested));
        assert!(atlas.uploads().is_empty());
        assert_eq!(positions, requested.map(|rect| atlas.origin(rect)));
    }

    #[test]
    fn churn_reclaims_old_glyphs_without_evicting_any_current_frame_request() {
        let mut atlas = FontAtlas {
            side: 64,
            ..Default::default()
        };
        for frame in 0..1000u16 {
            let requested = [[frame, 0, frame + 25, 25], [frame, 30, frame + 25, 55]];
            assert!(atlas.prepare(&requested));
            for rect in requested {
                let [x, y] = atlas.origin(rect).unwrap();
                assert!(u32::from(x + 25 + FONT_ATLAS_GUTTER) <= atlas.side);
                assert!(u32::from(y + 25 + FONT_ATLAS_GUTTER) <= atlas.side);
            }
            assert!(atlas.resident.len() <= 4);
        }
    }

    #[test]
    fn a_frame_exceeding_the_bound_is_rejected_before_changing_residency() {
        let mut atlas = FontAtlas {
            side: 32,
            ..Default::default()
        };
        let first = [0, 0, 20, 20];
        assert!(atlas.prepare(&[first]));
        let origin = atlas.origin(first);
        assert!(!atlas.prepare(&[first, [20, 0, 40, 20]]));
        assert_eq!(atlas.origin(first), origin);
        assert!(atlas.uploads().is_empty());
        assert!(!atlas.prepare(&[[0, 0, 33, 1]]));
        assert!(!atlas.prepare(&[[3, 0, 2, 1]]));
    }
}
