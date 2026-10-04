//! Column shelter selection for native seasonal leaf materials.
//!
//! ClientLeavesSeasonColorUtils scans upward
//! to the height map, skipping air/leaves and native exempt blocks. TopSnow
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
        for (offset_y, chunk) in context.neighbourhood.seasonal_column() {
            let facts = PaletteFacts::new(
                context.classifier,
                context.visuals,
                context.network_id_mode,
                chunk,
            );
            if facts.is_air() {
                continue;
            }
            let Some(base_y) = offset_y.checked_mul(SIDE as i32) else {
                continue;
            };
            for (x, column) in highest_blocker.iter_mut().enumerate() {
                for (z, highest) in column.iter_mut().enumerate() {
                    for y in (0..SIDE).rev() {
                        if !facts.seasonal_shelters_at(x, y, z) {
                            continue;
                        }
                        if let Some(height) = base_y.checked_add(y as i32) {
                            *highest = (*highest).max(height);
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
