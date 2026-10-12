//! Column shelter selection for native seasonal leaf materials.
//!
//! Vanilla's seasonal leaf colour scans upward to the height map, skipping air/leaves and native exempt blocks. TopSnow
//! delegates to a non-air extra layer, otherwise only the full-height state
//! shelters leaves. Assets owns the shared predicate used by particle colours.

use super::models::PaletteResolutionContext;
use crate::{SIDE, contributors::PaletteFacts};

pub(crate) struct SeasonalCoverage {
    highest_blocker: [[i32; SIDE]; SIDE],
}

impl SeasonalCoverage {
    pub(crate) fn new(context: PaletteResolutionContext<'_, '_>) -> Self {
        let mut highest_blocker = [[i32::MIN; SIDE]; SIDE];
        // Sections are read from the top, so a column's first blocker is its highest and the
        // sections below it need not be read; the scan ends once every column has one.
        let mut sections: Vec<_> = context.neighbourhood.seasonal_column().collect();
        sections.sort_unstable_by_key(|&(offset_y, _)| std::cmp::Reverse(offset_y));
        let mut open = SIDE * SIDE;
        for (offset_y, chunk) in sections {
            if open == 0 {
                break;
            }
            let mut facts = PaletteFacts::new(
                context.classifier,
                context.visuals,
                context.network_id_mode,
                chunk,
            );
            if facts.is_air() {
                continue;
            }
            // A section's columns may each be read in full, which a decoded section answers fastest.
            facts.decode_voxels();
            let Some(base_y) = offset_y.checked_mul(SIDE as i32) else {
                continue;
            };
            for (x, column) in highest_blocker.iter_mut().enumerate() {
                for (z, highest) in column.iter_mut().enumerate() {
                    if *highest != i32::MIN {
                        continue;
                    }
                    for y in (0..SIDE).rev() {
                        if !facts.seasonal_shelters_at(x, y, z) {
                            continue;
                        }
                        if let Some(height) = base_y.checked_add(y as i32) {
                            *highest = height;
                            open -= 1;
                        }
                        break;
                    }
                }
            }
        }
        Self { highest_blocker }
    }

    pub(crate) fn exposed(&self, [x, y, z]: [usize; 3]) -> bool {
        self.highest_blocker[x][z] < y as i32
    }
}
