//! Shared admission and allocation ownership for every drawing contributor.

use crate::{GpuReservation, Pass, PrimitiveRanges, Primitives, RenderOutput};
use mod_api::{
    MAX_RENDER_BEAMS, MAX_RENDER_BILLBOARDS, MAX_RENDER_DECALS, MAX_RENDER_PASSES,
    MAX_RENDER_RIBBONS,
};
use std::sync::Arc;

/// Combines several mods' output in load order within the single-mod budgets: earlier mods
/// keep a contested pass name and fill each primitive budget first. Merged primitives keep
/// their identity while every source is unchanged, so pass-only changes rebuild no geometry.
#[derive(Clone, Debug, Default)]
pub struct RenderMerge {
    sources: Vec<Arc<Primitives>>,
    merged: Arc<Primitives>,
    admitted: Vec<PrimitiveRanges>,
}

impl RenderMerge {
    /// Combines current outputs within shared limits and preserves their allocation owners.
    pub fn merge<'a>(
        &mut self,
        outputs: impl IntoIterator<Item = &'a RenderOutput>,
    ) -> RenderOutput {
        let outputs: Vec<&RenderOutput> = outputs.into_iter().collect();
        let mut passes: Vec<Pass> = Vec::new();
        for pass in outputs.iter().flat_map(|output| &output.passes) {
            if passes.len() < MAX_RENDER_PASSES && passes.iter().all(|kept| kept.name != pass.name)
            {
                passes.push(pass.clone());
            }
        }
        passes.sort_by(|a, b| a.order.cmp(&b.order).then_with(|| a.name.cmp(&b.name)));
        let primitives = self.merge_primitives(&outputs);
        let masks = outputs
            .iter()
            .flat_map(|output| output.masks.iter().cloned())
            .collect();
        let retained = outputs
            .iter()
            .flat_map(|output| output.retained.iter().cloned())
            .collect();
        let gpu_reservations = outputs
            .iter()
            .zip(&self.admitted)
            .flat_map(|(output, admitted)| {
                output
                    .gpu_reservations
                    .iter()
                    .map(move |reservation| GpuReservation {
                        primitives: reservation.primitives.remap(admitted),
                        ..reservation.clone()
                    })
            })
            .collect();
        RenderOutput {
            passes,
            primitives,
            masks,
            retained,
            gpu_reservations,
        }
    }

    /// Admits each primitive once and records where each source survived in the shared list.
    fn merge_primitives(&mut self, outputs: &[&RenderOutput]) -> Arc<Primitives> {
        let same_sources = self.sources.len() == outputs.len()
            && self
                .sources
                .iter()
                .zip(outputs)
                .all(|(source, output)| Arc::ptr_eq(source, &output.primitives));
        if same_sources {
            return Arc::clone(&self.merged);
        }
        self.sources = outputs
            .iter()
            .map(|output| Arc::clone(&output.primitives))
            .collect();
        let mut merged = Primitives::default();
        self.admitted.clear();
        for other in outputs.iter().map(|output| &output.primitives) {
            self.admitted.push(PrimitiveRanges {
                decals: fill(&mut merged.decals, &other.decals, MAX_RENDER_DECALS),
                ribbons: fill(&mut merged.ribbons, &other.ribbons, MAX_RENDER_RIBBONS),
                beams: fill(&mut merged.beams, &other.beams, MAX_RENDER_BEAMS),
                billboards: fill(
                    &mut merged.billboards,
                    &other.billboards,
                    MAX_RENDER_BILLBOARDS,
                ),
            });
        }
        let mut drawing = outputs
            .iter()
            .filter(|output| !output.primitives.is_empty());
        if let (Some(only), None) = (drawing.next(), drawing.next())
            && *only.primitives == merged
        {
            self.merged = Arc::clone(&only.primitives);
        } else if merged != *self.merged {
            self.merged = Arc::new(merged);
        }
        Arc::clone(&self.merged)
    }
}

/// Appends the admitted prefix and returns its exact range in the shared list.
fn fill<T: Clone>(into: &mut Vec<T>, from: &[T], cap: usize) -> std::ops::Range<usize> {
    let start = into.len();
    let room = cap.saturating_sub(into.len());
    into.extend(from.iter().take(room).cloned());
    start..into.len()
}
