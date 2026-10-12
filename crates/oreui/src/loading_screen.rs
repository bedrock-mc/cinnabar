//! The OreUI loading screen and the motion effects that outlive the screens that started them.

use std::collections::HashMap;

use ui::{IconRef, LoadingStage, UiNode};

use super::{Look, OreUiScreen, paint::Canvas, progress, theme};
use ui_engine::engine::screen::{PaintError, PaintFrame};

/// What one loading paint reads.
pub struct LoadingInput<'a> {
    pub stage: LoadingStage,
    /// The title and detail lines.
    pub words: [&'a str; 2],
    pub size: [f32; 2],
    pub title_artwork: Option<IconRef>,
    /// The destination dimension's block icon while changing dimension.
    pub destination_icon: Option<IconRef>,
    pub artwork: &'a HashMap<String, IconRef>,
    pub dimension: i32,
}

impl OreUiScreen {
    /// Draws the join and dimension-change progress over the loading backdrop.
    pub fn paint_loading(
        &mut self,
        input: LoadingInput<'_>,
        frame: &mut PaintFrame<'_>,
    ) -> Result<(), PaintError> {
        let LoadingInput {
            stage,
            words,
            size,
            title_artwork,
            destination_icon,
            artwork,
            dimension,
        } = input;
        let originals = self
            .originals
            .as_deref()
            .filter(|_| self.look == Look::Originals);
        let mut canvas = Canvas::new(
            &mut *frame.nodes,
            &mut *frame.next,
            &mut *frame.layouts,
            frame.font,
            frame.metrics,
            frame.solid_page,
            originals,
        );
        canvas.appearance = theme::Appearance::from_dark(self.dark_mode);
        canvas.seconds = frame.now;
        canvas.title_artwork = title_artwork;
        canvas.destination_icon = destination_icon;
        canvas.artwork = Some(artwork);
        self.transitions.begin_frame(None, false, frame.now);
        canvas.transitions = Some(&mut self.transitions);
        progress::draw(
            &mut canvas,
            None,
            size,
            &progress::Progress {
                title: words[0],
                detail: words[1],
                fraction: None,
                cancel: None,
                indicator: stage != LoadingStage::ChangingDimension,
                stage,
                destination: (stage == LoadingStage::ChangingDimension).then_some(dimension),
            },
        )
    }

    /// Plays out exit and capture effects over everything drawn this frame.
    pub fn finish_motion(
        &mut self,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        seconds: f64,
        size: [f32; 2],
    ) -> Result<(), PaintError> {
        self.transitions.effects.finish(nodes, next, seconds, size)
    }
}
