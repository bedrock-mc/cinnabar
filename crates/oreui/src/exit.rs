//! The quit confirmation, drawn as an OreUI modal over the current screen.

use super::modal::{self, Modal};
use super::paint::Canvas;
use super::theme::{Appearance, TEXT};
use super::widgets::Variant;
use super::{Look, OreUiScreen};
use launcher::menu::{MenuAction, MenuView};
use ui::UiRect;
use ui_engine::engine::screen::{PaintError, PaintFrame};
use ui_engine::scroll::ScrollArea;

/// The quit dialog's hit targets and scroll areas.
type ExitTargets = (Vec<(MenuAction, UiRect)>, Vec<ScrollArea>);

impl OreUiScreen {
    /// Draws the quit confirmation; returns its targets and its scroll areas.
    pub fn paint_exit(
        &mut self,
        view: &MenuView,
        size: [f32; 2],
        translate: &dyn Fn(&str) -> Option<std::sync::Arc<str>>,
        frame: &mut PaintFrame<'_>,
    ) -> Result<ExitTargets, PaintError> {
        let originals = self
            .originals
            .as_deref()
            .filter(|_| self.look == Look::Originals);
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
            &mut *frame.nodes,
            &mut *frame.next,
            &mut *frame.layouts,
            frame.font,
            frame.metrics,
            frame.solid_page,
            originals,
        );
        canvas.appearance = Appearance::from_dark(view.settings_options.oreui_dark_mode());
        canvas.seconds = frame.now;
        canvas.transitions = Some(&mut self.transitions);
        modal::draw(&mut canvas, view, size, &dialog)?;
        Ok((canvas.hits, canvas.scrolls))
    }
}

#[cfg(test)]
mod tests {
    use super::super::theme;
    use ui_engine::test_support::fixture_font;
    use ui_engine::text_metrics::TextMetrics;
    use {
        super::*,
        launcher::menu::{MenuAction, MenuView},
        ui::{SafeArea, TextLayoutCache, UiNode},
    };

    fn frame(
        screen: &mut OreUiScreen,
        visible: bool,
        seconds: f64,
    ) -> (Vec<UiNode>, Vec<(MenuAction, UiRect)>) {
        let size = [1280.0, 720.0];
        screen.transitions.begin_frame(None, false, seconds);
        let metrics =
            TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2));
        let font = fixture_font();
        let mut layouts = TextLayoutCache::new(32, 1024 * 1024);
        let (mut nodes, mut next) = (Vec::new(), 1);
        let hits = if visible {
            let mut frame = PaintFrame {
                layouts: &mut layouts,
                font: &font,
                metrics,
                solid_page: 0,
                safe_area: SafeArea::ZERO,
                content: size,
                translate: &|_| None,
                language: [0; 3],
                now: seconds,
                nodes: &mut nodes,
                next: &mut next,
                renderers: &mut [],
            };
            screen
                .paint_exit(
                    &MenuView::new(true, "Player".into()),
                    size,
                    &|_| None,
                    &mut frame,
                )
                .unwrap()
                .0
        } else {
            Vec::new()
        };
        screen
            .finish_motion(&mut nodes, &mut next, seconds, size)
            .unwrap();
        screen.transitions.end_frame();
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
        let mut screen = OreUiScreen::default();
        frame(&mut screen, false, 0.0);
        let (opening, hits) = frame(&mut screen, true, 1.0);
        let (settled, settled_hits) = frame(&mut screen, true, 1.2);
        assert_eq!(hits, settled_hits);
        let start = quit_face(&opening).unwrap();
        let end = quit_face(&settled).unwrap();
        assert!(start.0.min().y() > end.0.min().y());
        assert!(start.1 < end.1);
        assert_eq!(end.1, 255);
        let (_, closing_hits) = frame(&mut screen, false, 2.0);
        assert!(closing_hits.is_empty());
        let (closing, _) = frame(&mut screen, false, 2.04);
        let leaving = quit_face(&closing).unwrap();
        assert!(leaving.1 > 0 && leaving.1 < end.1);
        assert!(leaving.0.min().y() > end.0.min().y());
        assert!(quit_face(&frame(&mut screen, false, 2.2).0).is_none());
    }

    #[test]
    fn disabling_motion_settles_quit_immediately_and_removes_it_on_cancel() {
        let mut screen = OreUiScreen::default();
        screen.transitions.configure_motion(false);
        let (nodes, _) = frame(&mut screen, true, 0.0);
        assert_eq!(quit_face(&nodes).unwrap().1, 255);
        assert!(frame(&mut screen, false, 0.001).0.is_empty());
    }
}
