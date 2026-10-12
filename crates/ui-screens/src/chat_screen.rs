//! The open chat through the built-in `chat.chat_screen`: history as the screen
//! controller's `messages_factory`, the edit box, command suggestions and usage
//! lines as the `auto_complete` collection, and the send and back buttons.
//! Editing, completion and sending stay with the client's runtime.

use std::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
};

use json_ui::{
    CollectionItem, DataSource, FactoryItem, HitKind, HitRegion, Scalar, ScrollMetrics, ViewState,
};
use serde_json::Value;
use ui::{FONT_DESIGN_PIXEL_TEXELS, UiPoint, UiRect};
use ui_engine::cached_screen::CachedScreen;
use ui_engine::engine::{
    FormEngine, ScreenArt,
    screen::{PaintError, PaintFrame, ScreenRegistry},
};

use crate::chat_coordinates::ChatCoordinates;
use crate::regions::{segments, window_rect};
use crate::settings_chat::ChatSettings;
use crate::text::{bounded_visible_text, resolve_chat_line};

pub const CHAT_SCREEN: &str = "chat.chat_screen";

/// The open chat keeps the built-in presentation whatever server packs layer.
pub fn register(registry: &mut ScreenRegistry) {
    registry.builtin_screen(CHAT_SCREEN);
}

/// The messages factory's `max_children_size`.
const MAX_MESSAGES: usize = 100;
/// The edit box caret's on and off time.
const CARET_BLINK_MILLIS: u64 = 500;
const MESSAGES_VIEW: &str = "messages_panel";

/// What a pointer press on the chat screen means.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChatHit {
    Suggestion(usize),
    Link(usize),
    ExtensionAction(usize),
    LinkOpen,
    LinkCancel,
    Send,
    CopyCoordinates,
    Paste,
    CoordinateDropdown,
    CoordinateSource(bool),
    Close,
    SettingsOpen,
    SettingsClose,
    SettingsAction(launcher::menu::MenuAction),
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct LinkGeneration {
    passes: usize,
    scale: u32,
    origin: [u32; 2],
    font: assets::FontCatalogIdentity,
}

/// The chat screen's cached layout and last frame's input geometry.
#[derive(Default)]
pub struct ChatScreen {
    links: Vec<String>,
    extension_events: Vec<String>,
    pending_link: Option<String>,
    link_hits: Vec<(ChatHit, UiRect, String)>,
    link_generation: Option<LinkGeneration>,
    pub settings: ChatSettings,
    pub coordinates: ChatCoordinates,
    screen: CachedScreen,
    /// Window-logical hit rects and their layout keys, from the last frame.
    hits: Vec<(ChatHit, UiRect, String)>,
    edit_box: Option<String>,
    /// The messages view's key and extents from the last frame.
    scroll: Option<(String, ScrollMetrics)>,
    /// Virtual px scrolled up from the newest message.
    from_bottom: f64,
    messages: usize,
    open: bool,
    /// Logical px per virtual px, from the last frame.
    scale: f32,
    pointer: Option<UiPoint>,
}

/// What one chat paint reads from the client's chat state.
pub struct ChatInput<'a> {
    pub editor: &'a ui::ChatEditor,
    /// The history, oldest first.
    pub messages: &'a VecDeque<ui::ChatMessage>,
    pub suggestions: &'a [Arc<str>],
    pub usage: Option<&'a str>,
    /// The keyboard-selected suggestion.
    pub selected_suggestion: Option<usize>,
    /// Chat text the personal HUD turns into extension actions.
    pub annotations: &'a [ui::mod_hud::ChatAction],
    pub clocks: &'a BTreeMap<String, f64>,
    pub now_millis: u64,
}

impl ChatScreen {
    /// Paints the open chat, then its link confirmation over it, and records their input
    /// geometry.
    pub fn paint(
        &mut self,
        engine: &FormEngine,
        input: ChatInput<'_>,
        frame: &mut PaintFrame<'_>,
    ) -> Result<(), PaintError> {
        let messages = input.messages.len();
        // The view jumps to the newest message on open and on every update.
        if !self.open || messages != self.messages {
            self.from_bottom = 0.0;
        }
        self.open = true;
        self.messages = messages;
        let translate = frame.translate;
        let mut data = chat_data(&input, &self.settings, translate);
        crate::chat_coordinates::bind(self, &mut data, input.now_millis, &|key| {
            translate(key).map_or_else(|| key.to_owned(), |value| value.to_string())
        });
        crate::settings_chat::bind(&self.settings, &mut data, &|key| {
            translate(key).map_or_else(|| key.to_owned(), |value| value.to_string())
        });
        // The view's bag (`#scrolled_to_end`) as it stood last frame, for its view bindings.
        if let Some((key, metrics)) = &self.scroll
            && let Some(name) = key.rsplit('/').next()
        {
            data.set_control_values(name, metrics.feedback());
        }
        let view = self.view_state(input.selected_suggestion);
        let context =
            crate::chat_position::context(engine.context().clone(), &self.settings.options);
        let catalog = Arc::clone(engine.screen_catalog(CHAT_SCREEN));
        let metrics = frame.metrics;
        let px = metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
        let language = frame.language;
        let art = ScreenArt {
            clocks: Some(input.clocks),
            view: Some(&view),
            edit: Some(ui_engine::engine::host_edit::Feedback::from_chat(
                input.editor,
            )),
            ..ScreenArt::default()
        };
        let screen = &mut self.screen;
        let drawn = engine.draw(art, frame, |env, root| {
            screen.render_with(
                CHAT_SCREEN,
                &catalog,
                &context,
                data,
                (root, px, language),
                env,
                &view,
            )
        })?;
        self.hits.clear();
        self.edit_box = None;
        let Some(drawn) = drawn else {
            return Ok(());
        };
        self.scale = drawn.scale;
        self.scroll = drawn
            .report
            .scrolls
            .iter()
            .find(|(key, _)| key.contains(MESSAGES_VIEW))
            .map(|(key, metrics)| (key.clone(), metrics.clone()));
        if !self.settings.open && self.pending_link.is_none() {
            let generation = LinkGeneration {
                passes: self.screen.passes,
                scale: drawn.scale.to_bits(),
                origin: drawn.origin.map(f32::to_bits),
                font: frame.font.identity(),
            };
            if self.link_generation != Some(generation) || !input.annotations.is_empty() {
                self.links.clear();
                self.link_hits.clear();
                for (url, bounds, key) in crate::chat_links::hits(
                    self.screen.nodes(),
                    &view,
                    metrics,
                    frame.font,
                    frame.layouts,
                    drawn.scale,
                    drawn.origin,
                    input.annotations,
                ) {
                    let index = self.links.len();
                    self.links.push(url);
                    let hit = if self.links[index].starts_with("extension-action:") {
                        ChatHit::ExtensionAction(index)
                    } else {
                        ChatHit::Link(index)
                    };
                    self.link_hits.push((hit, bounds, key));
                }
                self.link_generation = Some(generation);
            }
            self.hits.extend(self.link_hits.iter().cloned());
        }
        for region in drawn.hits.iter().filter(|region| region.enabled) {
            if region.kind == HitKind::EditBox {
                if !self.settings.open {
                    self.edit_box = Some(region.key.clone());
                }
                continue;
            }
            if let Some(actions) =
                crate::settings_controls::slider_actions(&self.settings.options, region)
            {
                for (step, bounds) in segments(region, actions.len(), drawn.scale, drawn.origin) {
                    self.hits.push((
                        ChatHit::SettingsAction(actions[step]),
                        bounds,
                        region.key.clone(),
                    ));
                }
                continue;
            }
            let hit = crate::settings_chat::action(&self.settings, region)
                .map(ChatHit::SettingsAction)
                .or_else(|| crate::chat_coordinates::action(region))
                .or_else(|| chat_hit(region));
            let Some(hit) = hit else {
                continue;
            };
            if self.settings.open
                && !matches!(hit, ChatHit::SettingsClose | ChatHit::SettingsAction(_))
            {
                continue;
            }
            if let Some(bounds) = window_rect(region, drawn.scale, drawn.origin) {
                self.hits.push((hit, bounds, region.key.clone()));
            }
        }
        self.paint_link_dialog(engine, frame)
    }

    /// Draws the confirmation for the retained link, whose buttons alone then take input.
    fn paint_link_dialog(
        &mut self,
        engine: &FormEngine,
        frame: &mut PaintFrame<'_>,
    ) -> Result<(), PaintError> {
        let Some(url) = self.pending_link.as_ref() else {
            return Ok(());
        };
        self.hits.clear();
        let model = json_ui::FormModel::Modal(json_ui::ModalForm {
            title: "Open link?".to_owned(),
            body: url.clone(),
            button1: "Open".to_owned(),
            button2: "Cancel".to_owned(),
        });
        let context = json_ui::form_context(&model, &crate::menu_screens::retail_context());
        let data = json_ui::form_data_source(&model);
        let state = json_ui::ViewState::default();
        let drawn = engine.render_screen(
            "popup_dialog.modal_dialog_popup",
            &data,
            &context,
            &state,
            ScreenArt::default(),
            frame,
        )?;
        if let Some(drawn) = drawn {
            for region in drawn.hits.iter().filter(|region| region.enabled) {
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
                if let Some(rect) = window_rect(region, drawn.scale, drawn.origin) {
                    self.hits.push((hit, rect, region.key.clone()));
                }
            }
        }
        Ok(())
    }

    /// Returns a displayed web target from the last chat frame.
    pub fn link(&self, index: usize) -> Option<&str> {
        self.links.get(index).map(String::as_str)
    }

    /// Forget the open chat's scroll and hover once the screen closes.
    pub fn close(&mut self) {
        self.open = false;
        self.settings.open = false;
        self.hits.clear();
        self.links.clear();
        self.pending_link = None;
        self.link_hits.clear();
        self.link_generation = None;
        self.pointer = None;
    }

    /// What a press at the window-logical `position` hits on the open chat.
    pub fn hit_test(&self, position: UiPoint) -> Option<ChatHit> {
        self.hits
            .iter()
            .rev()
            .find_map(|(hit, bounds, _)| bounds.contains(position).then_some(*hit))
    }

    /// Track the pointer for next frame's hover state.
    pub fn set_pointer(&mut self, position: Option<UiPoint>) {
        self.pointer = position;
    }

    /// Scroll the history by a wheel delta in notches, or logical px when
    /// `pixels`; positive scrolls toward older messages.
    pub fn scroll(&mut self, delta: f32, pixels: bool) {
        if self.settings.open || self.pending_link.is_some() {
            return;
        }
        let Some(metrics) = self.scroll.as_ref().map(|(_, metrics)| metrics.clone()) else {
            return;
        };
        let max = metrics.max_offset();
        self.from_bottom = if pixels {
            (self.from_bottom + f64::from(delta / self.scale.max(f32::EPSILON))).clamp(0.0, max)
        } else {
            let at = ScrollMetrics {
                offset: (max - self.from_bottom).clamp(0.0, max),
                ..metrics
            };
            max - at.offset_for_wheel(f64::from(delta))
        };
    }

    /// Retains the clicked target while the native popup owns chat input.
    pub fn request_link(&mut self, index: usize) {
        if self.pending_link.is_none() && !self.settings.open {
            self.pending_link = self.links.get(index).cloned();
            self.hits.clear();
        }
    }

    pub fn link_confirmation_open(&self) -> bool {
        self.pending_link.is_some()
    }

    pub fn cancel_link(&mut self) {
        self.pending_link = None;
        self.hits.clear();
    }

    /// Call only after the popup's explicit Open action.
    pub fn take_confirmed_link(&mut self) -> Option<String> {
        self.hits.clear();
        self.pending_link.take()
    }

    /// Records a native chat click as a bounded extension control event.
    pub fn request_extension_action(&mut self, index: usize) {
        if self.extension_events.len() >= ui::mod_panel::MAX_PANEL_EVENTS {
            return;
        }
        if let Some(action) = self
            .links
            .get(index)
            .and_then(|link| link.strip_prefix("extension-action:"))
        {
            self.extension_events.push(action.to_owned());
        }
    }

    /// Consumes native chat actions once; the caller routes them to their cosmetic owner.
    pub fn take_extension_actions(&mut self) -> Vec<String> {
        std::mem::take(&mut self.extension_events)
    }

    /// Drops link hits and pending extension events whose chat actions were withdrawn.
    pub fn forget_annotations(&mut self) {
        self.link_generation = None;
        self.link_hits.clear();
        self.extension_events.clear();
    }

    /// The open chat's last laid-out draw nodes, in virtual px.
    pub fn nodes(&self) -> &[json_ui::DrawNode] {
        self.screen.nodes()
    }

    /// The open chat's hit rects from the last frame.
    pub fn hits(&self) -> Vec<(ChatHit, UiRect)> {
        self.hits
            .iter()
            .map(|(hit, bounds, _)| (*hit, *bounds))
            .collect()
    }

    fn view_state(&self, selected: Option<usize>) -> ViewState {
        let hovered = self
            .pointer
            .and_then(|point| {
                self.hits
                    .iter()
                    .rev()
                    .find(|(_, bounds, _)| bounds.contains(point))
            })
            .or_else(|| {
                // The keyboard-selected suggestion shows its focus border.
                let selected = selected?;
                self.hits
                    .iter()
                    .find(|(hit, ..)| *hit == ChatHit::Suggestion(selected))
            })
            .map(|(_, _, key)| key.clone());
        let mut view = ViewState {
            hovered,
            focused: self.edit_box.clone(),
            ..ViewState::default()
        };
        // The view's `jump_to_bottom_on_update` jumps to the newest line whenever
        // its maximum changes; otherwise it keeps `from_bottom`.
        if let Some((key, metrics)) = &self.scroll {
            let max = metrics.max_offset();
            view.scroll
                .insert(key.clone(), (max - self.from_bottom).max(0.0));
            view.scroll_state.insert(
                key.clone(),
                json_ui::ScrollRetained {
                    extent: Some(max),
                    ..Default::default()
                },
            );
        }
        view
    }
}

/// What the chat controller binds, from the client's chat state.
fn chat_data(
    input: &ChatInput<'_>,
    settings: &ChatSettings,
    translate: &dyn Fn(&str) -> Option<Arc<str>>,
) -> DataSource {
    let settings = &*settings.options;
    let mut data = DataSource::new();
    data.set_strict(true);
    let text = |key: &str, fallback: &str| {
        Scalar::Text(translate(key).map_or_else(|| fallback.to_owned(), |text| text.to_string()))
    };
    let editor = input.editor;
    let mut content = String::with_capacity(editor.as_str().len() + 1);
    content.push_str(&editor.as_str()[..editor.cursor_byte()]);
    if editor.selection().is_none() && (input.now_millis / CARET_BLINK_MILLIS).is_multiple_of(2) {
        content.push('|');
    }
    content.push_str(&editor.as_str()[editor.cursor_byte()..]);
    data.set_global("#message_text_box_content", Scalar::Text(content));
    for flag in ["#send_button_visible", "#chat_title_visible"] {
        data.set_global(flag, Scalar::Bool(true));
    }
    data.set_global("#chat_title_text", text("chat.title", "Chat"));
    data.set_global(
        "#back_button_text",
        text("controller.buttonTip.back", "Back"),
    );
    data.set_global(
        "#send_button_accessibility_text",
        text("accessibility.chat.tts.send", ""),
    );
    let messages = input.messages;
    let items = messages
        .iter()
        .skip(messages.len().saturating_sub(MAX_MESSAGES))
        .filter(|_| settings.value("hide_chat") == 0)
        .map(|line| {
            let text = resolve_chat_line(line, translate);
            FactoryItem::new("chat_screen_messages", 0.0)
                .value(
                    "#text",
                    Scalar::Text(crate::settings_chat::message_text(settings, text.as_ref())),
                )
                .var(
                    "chat_font_type",
                    Value::from(
                        if !settings.chat_smooth_available() || settings.value("chat_typeface") == 0
                        {
                            "default"
                        } else {
                            "smooth"
                        },
                    ),
                )
                .var(
                    "chat_font_scale_factor",
                    Value::from(settings.chat_font_scale()),
                )
                .var(
                    "chat_line_spacing",
                    Value::from(settings.chat_line_padding()),
                )
        })
        .collect();
    data.set_factory("messages_factory", items);
    let row = |text: &str, suggestion: bool| {
        CollectionItem::default()
            .with(
                "#auto_complete_text",
                Scalar::Text(bounded_visible_text(text).to_owned()),
            )
            .with("#is_autocomplete_suggestion", Scalar::Bool(suggestion))
    };
    // Suggestions list upward from the edit box; the usage line sits nearest it.
    let mut rows: Vec<CollectionItem> = input
        .suggestions
        .iter()
        .map(|suggestion| row(suggestion, true))
        .collect();
    rows.extend(input.usage.map(|usage| row(usage, false)));
    // The history's `#chat_visible` gives way to the suggestion list.
    data.set_global("#chat_visible", Scalar::Bool(rows.is_empty()));
    data.set_collection("auto_complete", rows);
    data
}

fn chat_hit(region: &HitRegion) -> Option<ChatHit> {
    match region.pressed.as_deref()? {
        "button.open_chat_settings" => Some(ChatHit::SettingsOpen),
        "button.close_chat_settings" => Some(ChatHit::SettingsClose),
        "button.click_autocomplete" => region.collection_index.map(ChatHit::Suggestion),
        "button.send" => Some(ChatHit::Send),
        "button.menu_exit" | "button.menu_cancel" | "button.chat_menu_cancel" => {
            Some(ChatHit::Close)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests;
