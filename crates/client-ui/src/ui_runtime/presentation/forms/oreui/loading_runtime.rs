use ui::UiNode;

use super::super::super::{TextMetrics, UiPresentationError, UiPresentationRuntime};
use super::{Look, paint::Canvas, progress, theme};

impl UiPresentationRuntime {
    pub(in super::super::super) fn append_oreui_motion(
        &mut self,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        size: [f32; 2],
    ) -> Result<(), UiPresentationError> {
        self.form_presentation.oreui_transitions.effects.finish(
            nodes,
            next,
            self.menu_seconds,
            size,
        )
    }

    pub(in super::super::super) fn append_oreui_loading(
        &mut self,
        stage: super::super::loading_screen::LoadingStage,
        words: [&str; 2],
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        size: [f32; 2],
    ) -> Result<(), UiPresentationError> {
        use super::super::loading_screen::LoadingStage;
        let originals = self
            .form_presentation
            .oreui_originals
            .as_deref()
            .filter(|_| self.form_presentation.oreui_look == Look::Originals);
        let title_artwork = self
            .form_presentation
            .engine
            .as_deref()
            .and_then(|engine| engine.menu_title(&self.menu_artwork.refs));
        let destination_icon = (stage == LoadingStage::ChangingDimension)
            .then(|| {
                crate::ui_runtime::oreui_assets::dimensions::destination(self.hud_frame.dimension)
                    .and_then(|art| self.item_icon(art.block, 0))
            })
            .flatten();
        let mut canvas = Canvas::new(
            nodes,
            next,
            &mut self.layouts,
            &self.font,
            metrics,
            self.solid_texture_page,
            originals,
        );
        canvas.appearance = theme::Appearance::from_dark(self.form_presentation.oreui_dark_mode);
        canvas.seconds = self.menu_seconds;
        canvas.title_artwork = title_artwork;
        canvas.destination_icon = destination_icon;
        canvas.artwork = Some(&self.menu_artwork.refs);
        self.form_presentation
            .oreui_transitions
            .begin_frame(None, false, self.menu_seconds);
        canvas.transitions = Some(&mut self.form_presentation.oreui_transitions);
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
                destination: (stage == LoadingStage::ChangingDimension)
                    .then_some(self.hud_frame.dimension),
            },
        )
    }
}
