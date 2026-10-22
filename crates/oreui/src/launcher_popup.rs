//! Launcher questions drawn with the shared OreUI modal.

use super::{
    Look, OreUiScreen,
    modal::{self, Modal},
    paint::Canvas,
    theme,
    widgets::Variant,
};
use launcher::menu::{MenuView, dialogs::Popup};
use ui_engine::{
    engine::screen::{PaintError, PaintFrame, ScreenOutput},
    menu_scroll::MenuScrolls,
    scroll::ScrollArea,
};

impl OreUiScreen {
    /// Paints a client-owned question and returns only its own focus and hit targets.
    pub fn paint_popup(
        &mut self,
        view: &MenuView,
        popup: &Popup,
        size: [f32; 2],
        scrolls: &MenuScrolls<launcher::menu::MenuAction>,
        frame: &mut PaintFrame<'_>,
    ) -> Result<(ScreenOutput<launcher::menu::MenuAction>, Vec<ScrollArea>), PaintError> {
        self.configure_accessibility(&view.settings_options);
        let originals = self
            .originals
            .as_deref()
            .filter(|_| self.look == Look::Originals);
        let mut canvas = Canvas::new(
            frame.nodes,
            frame.next,
            frame.layouts,
            frame.font,
            self.accessibility_metrics(frame.metrics),
            frame.solid_page,
            originals,
        );
        canvas.appearance = theme::Appearance::from_dark(view.settings_options.oreui_dark_mode());
        canvas.capture_focus = true;
        canvas.offsets = scrolls.offsets().clone();
        canvas.seconds = frame.now;
        canvas.transitions = Some(&mut self.transitions);
        let mut buttons = vec![(
            popup.button1.as_str().into(),
            if matches!(
                popup.confirm,
                launcher::menu::MenuAction::ConfirmRemoveShaderPack(..)
            ) {
                Variant::Destructive
            } else {
                Variant::Primary
            },
            Some(popup.confirm),
        )];
        if popup.dismiss != popup.confirm {
            buttons.push((
                popup.button2.as_str().into(),
                Variant::Secondary,
                Some(popup.dismiss),
            ));
        }
        modal::draw(
            &mut canvas,
            view,
            size,
            &Modal {
                title: &popup.title,
                body: popup.body.as_str().into(),
                body_color: theme::TEXT,
                items: Vec::new(),
                close: None,
                buttons,
            },
        )?;
        let focus = canvas
            .focus_hits
            .iter()
            .map(|(action, _)| *action)
            .collect();
        Ok((
            ScreenOutput {
                hits: canvas.hits,
                focus: Some(focus),
                frame: None,
            },
            canvas.scrolls,
        ))
    }
}
