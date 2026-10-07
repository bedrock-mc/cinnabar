use super::super::super::{TextMetrics, UiPresentationError, UiPresentationRuntime};
use super::modal::{self, Modal};
use super::paint::Canvas;
use super::theme::{Appearance, TEXT};
use super::widgets::Variant;
use crate::menu::{MenuAction, MenuView};
use ui::{UiNode, UiRect};

impl UiPresentationRuntime {
    pub(in super::super) fn append_oreui_exit(
        &mut self,
        view: &MenuView,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        size: [f32; 2],
        translate: &dyn Fn(&str) -> Option<std::sync::Arc<str>>,
    ) -> Result<Vec<(MenuAction, UiRect)>, UiPresentationError> {
        let originals = self
            .form_presentation
            .oreui_originals
            .as_deref()
            .filter(|_| self.form_presentation.oreui_look == super::Look::Originals);
        let quit = translate("globalPauseScreen.quit").unwrap_or_else(|| "Quit".into());
        let cancel = translate("gui.cancel").unwrap_or_else(|| "Cancel".into());
        let title = format!("Quit {}?", launcher::PRODUCT_NAME);
        let dialog = Modal {
            title: &title,
            items: Vec::new(),
            body: "Close the client and return to your desktop.".into(),
            body_color: TEXT,
            buttons: vec![
                (
                    quit.to_string().into(),
                    Variant::Destructive,
                    Some(MenuAction::ConfirmExit),
                ),
                (
                    cancel.to_string().into(),
                    Variant::Secondary,
                    Some(MenuAction::DismissDialog),
                ),
            ],
            close: Some(MenuAction::DismissDialog),
        };
        let mut canvas = Canvas::new(
            nodes,
            next,
            &mut self.layouts,
            &self.font,
            metrics,
            self.solid_texture_page,
            originals,
        );
        canvas.appearance = Appearance::from_dark(view.settings_options.oreui_dark_mode());
        canvas.seconds = self.menu_seconds;
        canvas.transitions = Some(&mut self.form_presentation.oreui_transitions);
        modal::draw(&mut canvas, view, size, &dialog)?;
        self.menu_scrolls.set_areas(canvas.scrolls);
        Ok(canvas.hits)
    }
}

#[cfg(test)]
mod tests {
    use super::super::theme;
    use super::*;
    use crate::ui_runtime::presentation::tests::fixture_font;

    fn frame(
        runtime: &mut UiPresentationRuntime,
        visible: bool,
        seconds: f64,
    ) -> (Vec<UiNode>, Vec<(MenuAction, UiRect)>) {
        let size = [1280.0, 720.0];
        runtime.menu_seconds = seconds;
        runtime
            .form_presentation
            .oreui_transitions
            .begin_frame(None, false, seconds);
        let metrics =
            TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2));
        let (mut nodes, mut next) = (Vec::new(), 1);
        let hits = if visible {
            runtime
                .append_oreui_exit(
                    &MenuView::new(true, "Player".into()),
                    &mut nodes,
                    &mut next,
                    metrics,
                    size,
                    &|_| None,
                )
                .unwrap()
        } else {
            Vec::new()
        };
        runtime
            .append_oreui_motion(&mut nodes, &mut next, size)
            .unwrap();
        runtime.end_animation_frame();
        (nodes, hits)
    }

    fn quit_face(nodes: &[UiNode]) -> Option<(UiRect, u8)> {
        nodes.iter().find_map(|node| match node.visual() {
            ui::UiVisual::Solid { color, .. } if color[..3] == theme::DESTRUCTIVE.fill[..3] => {
                Some((node.bounds(), color[3]))
            }
            _ => None,
        })
    }

    #[test]
    fn quit_dialog_enters_and_exits_with_visuals_but_keeps_click_targets_fixed() {
        let mut runtime = UiPresentationRuntime::new(fixture_font()).unwrap();
        frame(&mut runtime, false, 0.0);
        let (opening, hits) = frame(&mut runtime, true, 1.0);
        let (settled, settled_hits) = frame(&mut runtime, true, 1.2);
        assert_eq!(hits, settled_hits);
        let start = quit_face(&opening).unwrap();
        let end = quit_face(&settled).unwrap();
        assert!(start.0.min().y() > end.0.min().y());
        assert!(start.1 < end.1);
        assert_eq!(end.1, 255);
        let (_, closing_hits) = frame(&mut runtime, false, 2.0);
        assert!(closing_hits.is_empty());
        let (closing, _) = frame(&mut runtime, false, 2.04);
        let leaving = quit_face(&closing).unwrap();
        assert!(leaving.1 > 0 && leaving.1 < end.1);
        assert!(leaving.0.min().y() > end.0.min().y());
        assert!(quit_face(&frame(&mut runtime, false, 2.2).0).is_none());
    }

    #[test]
    fn disabling_motion_settles_quit_immediately_and_removes_it_on_cancel() {
        let mut runtime = UiPresentationRuntime::new(fixture_font()).unwrap();
        runtime
            .form_presentation
            .oreui_transitions
            .configure_motion(false);
        let (nodes, _) = frame(&mut runtime, true, 0.0);
        assert_eq!(quit_face(&nodes).unwrap().1, 255);
        assert!(frame(&mut runtime, false, 0.001).0.is_empty());
    }
}
