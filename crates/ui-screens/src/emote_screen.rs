//! Native wheel and native equip-slot popup, backed by the original local catalog.
use std::{collections::HashMap, sync::Arc};

use actor_animation::CustomEmote;
use json_ui::{DataSource, HitKind, InputMode, Scalar, ViewState};
use ui::{FONT_DESIGN_PIXEL_TEXELS, IconRef, UiPoint};
use ui_engine::cached_screen::CachedScreen;
use ui_engine::engine::{
    FormEngine, ScreenArt,
    screen::{PaintError, PaintFrame},
};
use ui_engine::frame::EngineFrame;

use crate::regions::window_rect;

pub const EMOTE_SCREEN: &str = "persona_emote.emote_wheel_screen";
pub const EMOTE_EQUIP_POPUP: &str = "persona_popups.popup_dialog__emote_equip_slot_editor";
const EMOTE_PREVIEW_TEXTURE: &str = "textures/ui/cinnabar_emote_preview";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmoteHit {
    Slot(usize),
    ChangeEmotes,
    Close,
}

/// The wheel's cached layout, last frame, pointer and input device.
#[derive(Default)]
pub struct EmoteScreen {
    screen: CachedScreen,
    frame: Option<EngineFrame>,
    pointer: Option<UiPoint>,
    input_mode: InputMode,
}

/// The emote wheel or equip popup, as the client's emote state shows it.
pub struct EmoteInput<'a> {
    pub equipping: bool,
    pub selected_slot: Option<usize>,
    pub slots: &'a [Option<CustomEmote>],
    /// The live player preview the occupied slots show.
    pub preview: Option<IconRef>,
}

impl EmoteScreen {
    /// Paints the wheel, or the equip popup while equipping.
    pub fn paint(
        &mut self,
        engine: &FormEngine,
        input: EmoteInput<'_>,
        frame: &mut PaintFrame<'_>,
    ) -> Result<(), PaintError> {
        let images: HashMap<_, _> = input
            .preview
            .into_iter()
            .map(|icon| (EMOTE_PREVIEW_TEXTURE.to_owned(), icon))
            .collect();
        let hovered = self.pointer.and_then(|point| {
            let frame = self.frame.as_ref()?;
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
        let focused = self.frame.as_ref().and_then(|frame| {
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
        let translate = frame.translate;
        let data = emote_data(&input, input.preview.is_some(), self.input_mode, &|key| {
            translate(key).map(|text| text.to_string())
        });
        let context = engine.context().clone();
        let catalog = Arc::clone(engine.catalog());
        let px = frame.metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
        let language = frame.language;
        let art = ScreenArt {
            view: Some(&view),
            images: Some(&images),
            ..Default::default()
        };
        let reference = if input.equipping {
            EMOTE_EQUIP_POPUP
        } else {
            EMOTE_SCREEN
        };
        let screen = &mut self.screen;
        self.frame = engine.draw(art, frame, |env, root| {
            screen.render_with(
                reference,
                &catalog,
                &context,
                data,
                (root, px, language),
                env,
                &view,
            )
        })?;
        Ok(())
    }

    /// What a press at `point` hits on the last painted wheel.
    pub fn hit_test(&self, point: UiPoint) -> Option<EmoteHit> {
        let frame = self.frame.as_ref()?;
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

    pub fn set_pointer(&mut self, pointer: Option<UiPoint>) {
        self.pointer = pointer;
    }

    /// Last active device; stationary cursor coordinates preserve modality.
    pub fn set_input_mode(&mut self, mode: InputMode) {
        self.input_mode = mode;
    }

    pub fn frame(&self) -> Option<&EngineFrame> {
        self.frame.as_ref()
    }

    pub fn close(&mut self) {
        self.frame = None;
        self.pointer = None;
    }
}

fn emote_data(
    state: &EmoteInput<'_>,
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
        ("#dressing_room_button_visible", !state.equipping),
    ] {
        data.set_global(name, Scalar::Bool(value));
    }
    let selected = state.selected_slot.and_then(|slot| state.slots[slot]);
    data.set_global(
        "#emote_name",
        Scalar::Text(selected.map_or(String::new(), |emote| emote.label().to_owned())),
    );
    let chosen = actor_animation::CustomEmote::ALL
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
            Scalar::Num(state.selected_slot.map_or(-1.0, |slot| slot as f64)),
        )]
        .into_iter()
        .collect(),
    );
    for (index, emote) in state.slots.iter().enumerate() {
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
