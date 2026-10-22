//! The built-in touch draft shares the mod editor's geometry, snapping and input handling.
use super::*;
mod factory;
use launcher::menu::{MenuView, settings_options::touch_layout::TouchLayout};
use ui::mod_hud::{Card, Cell, Hud};

/// Allows host placement deltas on actual controls while preserving their authored anchors.
pub(super) fn enable_placement(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(object) => {
            let control = object
                .get("$control")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|name| name.starts_with("cinnabar.touch."));
            let joystick = object.get("button_mappings").is_some_and(|mappings| {
                mappings.as_array().is_some_and(|items| {
                    items
                        .iter()
                        .any(|item| item["to_button_id"] == "cinnabar.touch.joystick")
                })
            });
            if control || joystick {
                object.insert("draggable".into(), serde_json::json!("both"));
            }
            for child in object.values_mut() {
                enable_placement(child);
            }
            // Packed items cannot translate themselves. Give movable buttons an unpacked
            // parent so each retains the row's factory geometry and can travel independently.
            if object.get("type").and_then(serde_json::Value::as_str) == Some("stack_panel")
                && let Some(children) = object
                    .get_mut("controls")
                    .and_then(serde_json::Value::as_array_mut)
            {
                for child in children {
                    let movable = child
                        .as_object()
                        .and_then(|named| named.iter().next())
                        .is_some_and(|(_, properties)| properties["draggable"] == "both");
                    if movable {
                        let name = child
                            .as_object()
                            .unwrap()
                            .keys()
                            .next()
                            .unwrap()
                            .split('@')
                            .next()
                            .unwrap()
                            .to_owned();
                        let button = std::mem::take(child);
                        *child = serde_json::json!({format!("{name}_slot"): {
                            "type":"panel", "size":["100%c","100%c"], "controls":[button]
                        }});
                    }
                }
            }
        }
        serde_json::Value::Array(items) => {
            for child in items {
                enable_placement(child);
            }
        }
        _ => {}
    }
}

impl UiPresentationRuntime {
    /// Paints actual touch artwork below the shared editor chrome, replacing only Settings.
    pub(in super::super::super) fn append_touch_editor(
        &mut self,
        runtime: &UiRuntime,
        view: &MenuView,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        content: [f32; 2],
    ) -> Result<(), UiPresentationError> {
        // Measure factory geometry even on a desktop host without gameplay touch controls.
        if self.screens.touch.editor.is_none() {
            let enabled = self.screens.touch.enabled;
            self.screens.touch.enabled = true;
            let rollback = (nodes.len(), *next);
            let measured = self.append_touch_controls(runtime, nodes, next, metrics, content);
            nodes.truncate(rollback.0);
            *next = rollback.1;
            self.screens.touch.enabled = enabled;
            measured?;
            let Some(base) = self.screens.touch.base.as_ref() else {
                return Ok(());
            };
            let anchors = factory::anchors();
            let cards = base
                .hits
                .iter()
                .filter_map(|hit| {
                    let id = control_id(hit.pressed.as_deref())?;
                    if id == touch::LOOK_SURFACE {
                        return None;
                    }
                    let name = hit.pressed.as_deref()?.strip_prefix("cinnabar.touch.")?;
                    let viewport = self.screens.touch.base_viewport.expect("measured viewport");
                    let anchor = *anchors.get(name).expect("authored touch control anchor");
                    let offset = anchor.offset_for_origin(
                        [hit.rect.x, hit.rect.y],
                        [hit.rect.w, hit.rect.h],
                        viewport.0.map(|v| f64::from(v / viewport.1)),
                    );
                    Some(Card {
                        id: name.into(),
                        editor_label: match name {
                            "joystick" => "Joystick",
                            "menu" => "Pause",
                            "chat" => "Chat",
                            "emote" => "Emotes",
                            "jump" => "Jump",
                            "sprint" => "Sprint",
                            "sneak" => "Sneak",
                            "attack" => "Attack",
                            "use" => "Use",
                            _ => name,
                        }
                        .into(),
                        anchor,
                        offset,
                        reset_anchor: Some(anchor),
                        reset_offset: Some(offset),
                        position: view.settings_options.touch_layout().position(name),
                        background_opacity: 0.,
                        background_radius: 0.,
                        width: hit.rect.w as f32,
                        cells: vec![Cell {
                            rect: [0., 0., hit.rect.w as f32, hit.rect.h as f32],
                            ..Cell::default()
                        }],
                        ..Card::default()
                    })
                })
                .collect();
            self.screens.touch.editor = Some(
                mod_ui::HudEditor::new(&Hud {
                    cards,
                    resizable: false,
                    ..Hud::default()
                })
                .expect("measured touch controls form a valid layout"),
            );
        }
        let translate = |key: &str| runtime.translation(key);
        let editor = self.screens.touch.editor.as_mut().expect("opened");
        let rollback = nodes.len();
        editor.paint(
            self.engine.as_deref(),
            &mut PaintFrame {
                layouts: &mut self.layouts,
                font: &self.font,
                metrics,
                solid_page: self.solid_texture_page,
                safe_area: self.safe_area,
                content,
                translate: &translate,
                language: runtime.text_generation(),
                now: self.menu_seconds,
                nodes,
                next,
                renderers: &mut [],
            },
            view.settings_options
                .value(launcher::menu::settings_options::SettingId::ScreenAnimations)
                != 0,
        );
        let chrome = nodes.split_off(rollback);
        self.append_touch_controls(runtime, nodes, next, metrics, content)?;
        nodes.extend(chrome);
        // Editor previews must never become gameplay activation regions.
        self.screens.touch.frame = None;
        Ok(())
    }
    /// Last painted control rectangle in window-logical coordinates for scripted UI captures.
    #[cfg(any(test, feature = "test-support"))]
    pub fn touch_editor_bounds(&self, name: &str) -> Option<[f32; 4]> {
        let (_, (content, px, safe)) = self
            .screens
            .touch
            .base
            .as_ref()
            .zip(self.screens.touch.base_viewport)?;
        let _ = content;
        let (_, rect) = self
            .screens
            .touch
            .editor
            .as_ref()?
            .rectangles()
            .find(|(id, _)| *id == name)?;
        Some([
            safe.left() + rect[0] as f32 * px,
            safe.top() + rect[1] as f32 * px,
            rect[2] as f32 * px,
            rect[3] as f32 * px,
        ])
    }
    /// Last painted editing action in window-logical coordinates for scripted captures.
    #[cfg(any(test, feature = "test-support"))]
    pub fn touch_editor_action_bounds(&self, action: &str) -> Option<[f32; 4]> {
        self.screens.touch.editor.as_ref()?.action_bounds(action)
    }
    /// Visible labeled drag handle in window-logical coordinates, including compact paging.
    #[cfg(any(test, feature = "test-support"))]
    pub fn touch_editor_handle_bounds(&self, name: &str) -> Option<[f32; 4]> {
        self.screens.touch.editor.as_ref()?.handle_bounds(name)
    }
    /// Current editor action, used to defer toolbar taps until the owning finger releases.
    pub fn touch_editor_action_at(&self, at: [f32; 2]) -> Option<String> {
        self.screens
            .touch
            .editor
            .as_ref()?
            .action_at(at)
            .map(str::to_owned)
    }
    /// Routes a single captured pointer in window-logical coordinates.
    pub fn touch_editor_pointer(&mut self, at: [f32; 2], pressed: bool, held: bool) {
        if let Some(editor) = &mut self.screens.touch.editor {
            editor.host_pointer(at, pressed, held);
        }
    }
    /// Cancels a finger independently from Save/Cancel for the whole draft.
    pub fn cancel_touch_editor_pointer(&mut self) {
        if let Some(editor) = &mut self.screens.touch.editor {
            editor.cancel_pointer();
        }
    }
    /// Samples Shift independently of key repetition for continuous snap bypass.
    pub fn touch_editor_modifiers(&mut self, modifiers: ui::mod_panel::FieldModifiers) {
        if let Some(editor) = &mut self.screens.touch.editor {
            editor.set_snap_bypass(modifiers.shift);
        }
    }
    /// Routes editor shortcuts without sending any editing key to gameplay.
    pub fn touch_editor_key(&mut self, key: &str, modifiers: ui::mod_panel::FieldModifiers) {
        if let Some(editor) = &mut self.screens.touch.editor {
            editor.host_key(key, modifiers);
        }
    }
    /// Drops an abandoned settings surface and all retained input geometry.
    pub fn close_touch_editor(&mut self) {
        self.screens.touch.editor = None;
    }
    /// A completed transaction: Save returns its layout; Cancel returns no replacement.
    pub fn take_touch_editor_result(&mut self) -> Option<Option<TouchLayout>> {
        let result = self.screens.touch.editor.as_mut()?.take_result()?;
        let layout = result.saved.then(|| {
            let mut layout = TouchLayout::default();
            {
                for placement in result.placements {
                    if let Some(position) = placement.position {
                        let factory = self
                            .screens
                            .touch
                            .base
                            .as_ref()
                            .zip(self.screens.touch.base_viewport)
                            .and_then(|(base, (content, px, _))| {
                                base.hits
                                    .iter()
                                    .find(|hit| {
                                        hit.pressed.as_deref()
                                            == Some(&format!("cinnabar.touch.{}", placement.id))
                                    })
                                    .map(|hit| {
                                        [hit.rect.x, hit.rect.y].into_iter().enumerate().all(
                                            |(axis, at)| {
                                                let size =
                                                    if axis == 0 { hit.rect.w } else { hit.rect.h };
                                                let travel =
                                                    (f64::from(content[axis] / px) - size).max(0.);
                                                (f64::from(position[axis]) * travel - at).abs()
                                                    < 0.01
                                            },
                                        )
                                    })
                            });
                        if factory != Some(true) {
                            layout.set(&placement.id, position);
                        }
                    }
                }
            }
            layout
        });
        self.close_touch_editor();
        Some(layout)
    }
}
