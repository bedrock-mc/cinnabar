//! Reuses a screen's last paint while everything it read is unchanged and its animations have
//! settled, repeating only the paint's bookkeeping: animation clocks and destroyed roots, mask
//! rasters and outlines it keeps resident.

use std::{collections::BTreeMap, sync::Arc};

use json_ui::{AnimationRecord, DrawNode, FormRender};
use ui::{SafeArea, UiNode};

use super::{Art, ScreenArt, masks::PreparedRasters, screen::PaintFrame};
use crate::{frame::EngineFrame, text_metrics::TextMetrics, textures::TextureRevision};

#[cfg(test)]
mod tests;

/// One screen's last paint, kept to be reused while nothing it was painted from has changed.
#[derive(Default)]
pub struct RetainedPaint {
    last: Option<Painted>,
    /// Paints run rather than reused, for cache tests and profiling.
    pub paints: usize,
}

/// A completed paint and everything it read that a later frame compares.
pub(super) struct Painted {
    pub(super) inputs: PaintInputs,
    /// The painted layout; holding it keeps a later layout from reusing its identity.
    pub(super) layout: Arc<[DrawNode]>,
    pub(super) textures: TextureRevision,
    pub(super) events_fired: u64,
    pub(super) animation: AnimationRecord,
    pub(super) rasters: PreparedRasters,
    /// The nodes the paint appended, mask sprites resolved.
    pub(super) nodes: Vec<UiNode>,
    pub(super) next: u32,
    pub(super) frame: EngineFrame,
}

/// The frame and art values a paint reads besides its layout, textures and animations.
pub(super) struct PaintInputs {
    /// The output's length and next node id when the paint began; ids and overlay ranges follow.
    start: (usize, u32),
    metrics: TextMetrics,
    solid_page: u16,
    safe_area: SafeArea,
    content: [f32; 2],
    language: [usize; 3],
    font: assets::FontCatalogIdentity,
    nearest_sampling: bool,
    pixel_snap_offset: [f64; 2],
    clocks: Option<BTreeMap<String, f64>>,
}

impl PaintInputs {
    /// What a paint into `frame` under `art`, beginning now, reads.
    pub(super) fn of(frame: &PaintFrame<'_>, art: &ScreenArt<'_>) -> Self {
        Self {
            start: (frame.nodes.len(), *frame.next),
            metrics: frame.metrics,
            solid_page: frame.solid_page,
            safe_area: frame.safe_area,
            content: frame.content,
            language: frame.language,
            font: frame.font.identity(),
            nearest_sampling: art.nearest_sampling,
            pixel_snap_offset: art.pixel_snap_offset,
            clocks: art.clocks.cloned(),
        }
    }

    fn matches(&self, frame: &PaintFrame<'_>, art: &ScreenArt<'_>) -> bool {
        self.start == (frame.nodes.len(), *frame.next)
            && self.metrics == frame.metrics
            && self.solid_page == frame.solid_page
            && self.safe_area == frame.safe_area
            && self.content == frame.content
            && self.language == frame.language
            && self.font == frame.font.identity()
            && self.nearest_sampling == art.nearest_sampling
            && self.pixel_snap_offset == art.pixel_snap_offset
            && self.clocks.as_ref() == art.clocks
    }
}

/// Whether a paint under `art` reads only state a retained paint compares: no pointer, hover,
/// focus, edit feedback, caller overlay, omitted controls, downloaded images or bounds observer.
pub(super) fn reusable(art: &ScreenArt<'_>) -> bool {
    art.paint_bounds.is_none()
        && art.omit_controls.is_empty()
        && art.view.is_none()
        && art.tooltip.is_none()
        && art.pointer.is_none()
        && art.images.is_none()
        && art.edit.is_none()
        && art.overlay.is_empty()
}

impl RetainedPaint {
    /// Drops the last paint, so the next one runs in full.
    pub fn forget(&mut self) {
        self.last = None;
    }

    /// Keeps a completed paint whose animations have all settled.
    pub(super) fn remember(&mut self, painted: Painted) {
        debug_assert!(!painted.animation.playing());
        self.last = Some(painted);
    }

    /// Appends the last paint's output to `frame` and repeats its bookkeeping, when `render`
    /// is the layout it painted and nothing else it read has changed; `None` paints afresh.
    pub(super) fn replay(
        &self,
        textures: Art<'_>,
        frame: &mut PaintFrame<'_>,
        art: ScreenArt<'_>,
        render: &FormRender,
    ) -> Option<EngineFrame> {
        let last = self.last.as_ref()?;
        if !Arc::ptr_eq(&last.layout, &render.nodes) || !last.inputs.matches(frame, &art) {
            return None;
        }
        let mut atlas = textures.set.lock();
        let mut animator = textures
            .animator
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if textures.set.revision(&atlas) != last.textures
            || animator.events_fired() != last.events_fired
            || !animator.can_replay(&last.animation)
        {
            return None;
        }
        // The paint read these outlines and published these rasters; doing so again keeps
        // them as recently used as painting would, and reveals any that went missing.
        for (key, size) in last.rasters.sources() {
            atlas.original_outline(key, size);
        }
        last.rasters.repeat(&mut atlas, textures.set);
        if textures.set.revision(&atlas) != last.textures {
            return None;
        }
        animator.replay(&last.animation, frame.now);
        frame.nodes.extend_from_slice(&last.nodes);
        *frame.next = last.next;
        Some(last.frame.clone())
    }
}
