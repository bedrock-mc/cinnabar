//! Native wheel and native equip-slot popup, backed by the original local catalog.
use std::{collections::HashMap, sync::Arc};

use json_ui::{DataSource, HitKind, InputMode, Scalar, ViewState};
use ui::{UiNode, UiPoint};

use super::super::{
    FONT_DESIGN_PIXEL_TEXELS, TextMetrics, UiPresentationError, UiPresentationRuntime,
};
use super::engine::{EngineInputs, EngineOutput, ScreenArt};
use super::hud::CachedScreen;
use super::menus::window_rect;
use crate::ui_runtime::{UiRuntime, emotes::EmoteState, forms::EngineFrame};

pub const EMOTE_SCREEN: &str = "persona_emote.emote_wheel_screen";
pub const EMOTE_EQUIP_POPUP: &str = "persona_popups.popup_dialog__emote_equip_slot_editor";
const EMOTE_PREVIEW_TEXTURE: &str = "textures/ui/cinnabar_emote_preview";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmoteHit {
    Slot(usize),
    ChangeEmotes,
    Close,
}

#[derive(Default)]
pub(super) struct EmoteScreen {
    screen: CachedScreen,
    frame: Option<EngineFrame>,
    pointer: Option<UiPoint>,
    input_mode: InputMode,
}

impl UiPresentationRuntime {
    pub(in super::super) fn append_emote_screen(
        &mut self,
        runtime: &UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        content: [f32; 2],
        now_millis: u64,
    ) -> Result<(), UiPresentationError> {
        let preview = self.player_preview_icon();
        let images: HashMap<_, _> = preview
            .into_iter()
            .map(|icon| (EMOTE_PREVIEW_TEXTURE.to_owned(), icon))
            .collect();
        // The native wheel authors image controls. Their dynamic texture marker
        // is replaced by the same animated player mesh used by the HUD doll.
        self.player_preview_view = super::super::player_preview::PreviewView::Hud;
        let Some(renderer) = self.form_presentation.engine.as_deref() else {
            return Ok(());
        };
        let state = runtime.emotes();
        let emote = &mut self.form_presentation.emote;
        let hovered = emote.pointer.and_then(|point| {
            let frame = emote.frame.as_ref()?;
            frame
                .hits
                .iter()
                .rev()
                .find(|region| {
                    window_rect(region, frame.scale, frame.origin)
                        .is_some_and(|bounds| bounds.contains(point))
                })
                .map(|region| region.key.clone())
        });
        let focused = emote.frame.as_ref().and_then(|frame| {
            frame
                .hits
                .iter()
                .find(|hit| hit.kind == HitKind::SelectionWheel)
                .map(|hit| hit.key.clone())
        });
        let view = ViewState {
            hovered,
            focused,
            ..Default::default()
        };
        let data = emote_data(state, preview.is_some(), emote.input_mode, &|key| {
            runtime.translation(key).map(|text| text.to_string())
        });
        let context = renderer.context().clone();
        let catalog = Arc::clone(renderer.catalog());
        let px = metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
        let translate = |key: &str| runtime.translation(key);
        let inputs = EngineInputs {
            layouts: &mut self.layouts,
            font: &self.font,
            metrics,
            solid_page: self.solid_texture_page,
            safe_area: self.safe_area,
            content,
            translate: &translate,
            language: runtime.text_generation(),
        };
        let out = EngineOutput {
            nodes,
            next,
            overlay: &[],
        };
        let art = ScreenArt {
            view: Some(&view),
            images: Some(&images),
            now: now_millis as f64 / 1_000.0,
            ..Default::default()
        };
        let reference = if state.is_equipping() {
            EMOTE_EQUIP_POPUP
        } else {
            EMOTE_SCREEN
        };
        let screen = &mut emote.screen;
        emote.frame = renderer.draw(art, inputs, out, |env, root| {
            screen.render_with(
                reference,
                &catalog,
                &context,
                data,
                (root, px, runtime.text_generation()),
                env,
                &view,
            )
        })?;
        Ok(())
    }

    pub fn hit_test_emote(&self, point: UiPoint) -> Option<EmoteHit> {
        let frame = self.form_presentation.emote.frame.as_ref()?;
        for region in frame.hits.iter().rev().filter(|region| region.enabled) {
            if !window_rect(region, frame.scale, frame.origin)
                .is_some_and(|bounds| bounds.contains(point))
            {
                continue;
            }
            if let Some(wheel) = &region.widget.selection_wheel {
                let point = [
                    f64::from((point.x() - frame.origin[0]) / frame.scale),
                    f64::from((point.y() - frame.origin[1]) / frame.scale),
                ];
                return wheel
                    .slice_at_rect(
                        [region.rect.x, region.rect.y, region.rect.w, region.rect.h],
                        point,
                    )
                    .map(EmoteHit::Slot);
            }
            match region.pressed.as_deref() {
                Some("button.dressing_room") => return Some(EmoteHit::ChangeEmotes),
                Some(
                    "button.menu_exit"
                    | "button.emote_wheel_exit_non_gamepad"
                    | "button.close_dialog"
                    | "button.close_emote_popup",
                ) => return Some(EmoteHit::Close),
                _ => {}
            }
        }
        None
    }

    pub fn set_emote_pointer(&mut self, pointer: Option<UiPoint>) {
        self.form_presentation.emote.pointer = pointer;
    }
    /// Last active device; stationary cursor coordinates preserve modality.
    pub fn set_emote_input_mode(&mut self, mode: InputMode) {
        self.form_presentation.emote.input_mode = mode;
    }
    pub fn emote_frame(&self) -> Option<&EngineFrame> {
        self.form_presentation.emote.frame.as_ref()
    }

    pub(in super::super) fn close_emote_screen(&mut self) {
        self.form_presentation.emote.frame = None;
        self.form_presentation.emote.pointer = None;
    }
}

fn emote_data(
    state: &EmoteState,
    preview_ready: bool,
    input_mode: InputMode,
    translate: &impl Fn(&str) -> Option<String>,
) -> DataSource {
    let mut data = DataSource::new();
    data.set_strict(true);
    for (name, value) in [
        ("#is_using_mouse", input_mode == InputMode::Mouse),
        ("#is_using_keyboard", input_mode == InputMode::Mouse),
        ("#is_touch_mode", input_mode == InputMode::Touch),
        ("#is_using_gamepad", input_mode == InputMode::Gamepad),
        (
            "#is_using_gamepad_override",
            input_mode == InputMode::Gamepad,
        ),
        ("#dressing_room_button_visible", !state.is_equipping()),
    ] {
        data.set_global(name, Scalar::Bool(value));
    }
    let selected = state.selected_slot().and_then(|slot| state.slots()[slot]);
    data.set_global(
        "#emote_name",
        Scalar::Text(selected.map_or(String::new(), |emote| emote.label().to_owned())),
    );
    let chosen = client_world::CustomEmote::ALL
        .first()
        .map_or("", |emote| emote.label());
    data.set_global("#emote_popup_title", Scalar::Text(chosen.to_owned()));
    for (name, key) in [
        (
            "#emote_screen_instructions",
            if input_mode == InputMode::Gamepad {
                "emote_wheel.gamepad_helper.select"
            } else {
                "emotes.instructions_keyboard"
            },
        ),
        ("#emote_screen_exit", "controller.buttonTip.back"),
    ] {
        data.set_global(name, Scalar::Text(translate(key).unwrap_or_default()));
    }
    data.set_control_values(
        "emote_wheel",
        [(
            "#hover_slice".to_owned(),
            Scalar::Num(state.selected_slot().map_or(-1.0, |slot| slot as f64)),
        )]
        .into_iter()
        .collect(),
    );
    for (index, emote) in state.slots().iter().enumerate() {
        data.set_indexed_global(index, "#emote_is_valid", Scalar::Bool(emote.is_some()));
        data.set_indexed_global(
            index,
            "#image_is_valid",
            Scalar::Bool(preview_ready && emote.is_some()),
        );
        data.set_indexed_global(
            index,
            "#emote_image",
            Scalar::Text(EMOTE_PREVIEW_TEXTURE.to_owned()),
        );
        data.set_indexed_global(
            index,
            "#emote_image_file_system",
            Scalar::Text("InAppPackage".to_owned()),
        );
        data.set_indexed_global(
            index,
            "#emote_index_name",
            Scalar::Text(emote.map_or(String::new(), |emote| emote.label().to_owned())),
        );
    }
    data
}

#[cfg(test)]
mod tests;
