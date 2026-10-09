//! Explicit confirmation before opening a chat URL in the platform browser.
use super::{
    super::{TextMetrics, UiPresentationError, UiPresentationRuntime},
    chat_screen::ChatHit,
    engine::{EngineInputs, EngineOutput, ScreenArt},
    menus::window_rect,
};
use crate::ui_runtime::UiRuntime;
use ui::UiNode;

impl UiPresentationRuntime {
    /// Retains the clicked target while the native popup owns chat input.
    pub fn request_chat_link(&mut self, index: usize) {
        let chat = &mut self.form_presentation.chat;
        if chat.pending_link.is_none() && !chat.settings.open {
            chat.pending_link = chat.links.get(index).cloned();
            chat.hits.clear();
        }
    }
    pub fn chat_link_confirmation_open(&self) -> bool {
        self.form_presentation.chat.pending_link.is_some()
    }
    pub fn cancel_chat_link(&mut self) {
        self.form_presentation.chat.pending_link = None;
        self.form_presentation.chat.hits.clear();
    }
    /// Call only after the popup's explicit Open action.
    pub fn take_confirmed_chat_link(&mut self) -> Option<String> {
        self.form_presentation.chat.hits.clear();
        self.form_presentation.chat.pending_link.take()
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn append_chat_link_dialog(
        &mut self,
        runtime: &UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        content: [f32; 2],
        now_millis: u64,
        refresh_sounds: bool,
    ) -> Result<(), UiPresentationError> {
        let Some(url) = self.form_presentation.chat.pending_link.as_ref() else {
            return Ok(());
        };
        self.form_presentation.chat.hits.clear();
        if refresh_sounds {
            self.form_presentation.chat.sounds.clear();
        }
        let Some(renderer) = self.form_presentation.engine.as_deref() else {
            return Ok(());
        };
        let model = json_ui::FormModel::Modal(json_ui::ModalForm {
            title: "Open link?".to_owned(),
            body: url.clone(),
            button1: "Open".to_owned(),
            button2: "Cancel".to_owned(),
        });
        let context = json_ui::form_context(&model, &super::menu_screens::retail_context());
        let data = json_ui::form_data_source(&model);
        let translate = |key: &str| runtime.translation(key);
        let state = json_ui::ViewState::default();
        let frame = renderer.render_screen(
            "popup_dialog.modal_dialog_popup",
            &data,
            &context,
            &state,
            ScreenArt {
                now: now_millis as f64 / 1000.0,
                ..Default::default()
            },
            EngineInputs {
                layouts: &mut self.layouts,
                font: &self.font,
                metrics,
                solid_page: self.solid_texture_page,
                safe_area: self.safe_area,
                content,
                translate: &translate,
                language: runtime.text_generation(),
            },
            EngineOutput {
                nodes,
                next,
                overlay: &[],
            },
        )?;
        if let Some(frame) = frame {
            for region in frame.hits.iter().filter(|region| region.enabled) {
                let hit = match region.pressed.as_deref() {
                    Some("popup_dialog.left_button") => ChatHit::LinkOpen,
                    Some(
                        "popup_dialog.rightcancel_button"
                        | "popup_dialog.escape"
                        | "button.menu_cancel"
                        | "button.menu_exit",
                    ) => ChatHit::LinkCancel,
                    _ => continue,
                };
                if let Some(rect) = window_rect(region, frame.scale, frame.origin) {
                    if refresh_sounds {
                        super::menu_sounds::collect(
                            region,
                            hit,
                            &mut self.form_presentation.chat.sounds,
                        );
                    }
                    self.form_presentation
                        .chat
                        .hits
                        .push((hit, rect, region.key.clone()));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pending_target_survives_frame_replacement_and_cancel_blocks_background_scroll() {
        let mut presentation =
            UiPresentationRuntime::new(super::super::super::tests::fixture_font()).unwrap();
        presentation
            .form_presentation
            .chat
            .links
            .push("https://example.com/a".into());
        presentation.request_chat_link(0);
        presentation.form_presentation.chat.links.clear();
        assert!(presentation.chat_link_confirmation_open());
        assert_eq!(
            presentation.take_confirmed_chat_link().as_deref(),
            Some("https://example.com/a")
        );
        assert!(!presentation.chat_link_confirmation_open());
        presentation
            .form_presentation
            .chat
            .links
            .push("https://example.com/b".into());
        presentation.request_chat_link(0);
        presentation.cancel_chat_link();
        assert_eq!(presentation.take_confirmed_chat_link(), None);
        presentation.request_chat_link(99);
        assert!(!presentation.chat_link_confirmation_open());
    }
}
