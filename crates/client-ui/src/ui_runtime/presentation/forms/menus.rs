//! Menus through the engine: each menu state opens its vanilla screen, and the
//! screen's pressed regions become the launcher's own hit targets, so the menu
//! state machine and its input path stay unchanged. States without a vanilla
//! screen, or a render that fails, fall back to the programmatic launcher.

use std::sync::Arc;

use json_ui::{HitRegion, ViewState};
use ui::{UiNode, UiRect};

use super::super::menu_scroll::ScrollArea;
use super::super::{TextMetrics, UiPresentationError, UiPresentationRuntime, menu, rect};
use super::{engine, menu_caret::TextSpot, menu_screens};
use crate::menu::{MenuAction, MenuView};
use crate::ui_runtime::{UiRuntime, forms::EngineFrame};

const MODAL_POPUP: &str = "popup_dialog.modal_dialog_popup";

/// A menu frame's hit targets and, for hover next frame, their region keys.
type MenuHits = (Vec<(MenuAction, UiRect)>, Vec<(MenuAction, String)>);

impl UiPresentationRuntime {
    /// Draw the visible menu and return its window-logical hit targets.
    pub fn append_menu(
        &mut self,
        runtime: &UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        width: f32,
        height: f32,
    ) -> Result<Vec<(MenuAction, UiRect)>, UiPresentationError> {
        self.menu_preview.begin_frame(
            self.menu_view
                .as_ref()
                .filter(|view| view.visible)
                .map(|view| view.screen),
        );
        if self
            .menu_view
            .as_ref()
            .is_some_and(|view| view.popup_open())
        {
            self.menu_preview.revoke_capture();
        }
        self.settings_slider_drag_targets.clear();
        self.form_presentation.menu_focus_context = None;
        self.form_presentation.menu_focus.clear();
        self.form_presentation.menu_focus_geometry.clear();
        self.form_presentation.menu_focus_landmarks.clear();
        self.form_presentation.oreui_slider_tracks.clear();
        self.form_presentation.oreui_settings_input = false;
        let Some(mut view) = self.menu_view.take() else {
            return Ok(Vec::new());
        };
        self.begin_menu_caret(Arc::make_mut(&mut view));
        if view.screen == crate::menu::MenuScreen::Death {
            Arc::make_mut(&mut view).death_reason = runtime.death_reason().to_owned();
        }
        let shown = view.as_ref();
        self.menu_scrolls.begin_frame(format!(
            "{:?}/{:?}/{:?}/{}",
            shown.screen, shown.server_tab, shown.profile_tab, shown.settings_section
        ));
        self.menu_scrolls.clear_areas();
        let drawn = if shown.visible {
            self.append_engine_menu(runtime, shown, nodes, next, metrics, width, height)
        } else {
            Ok(Some(Vec::new()))
        };
        let result = match drawn {
            Ok(Some(hits)) => Ok(hits),
            Ok(None) | Err(_) => {
                let owned_dialog = matches!(
                    shown.dialog,
                    Some(crate::menu::MenuDialog::Accounts | crate::menu::MenuDialog::Exit)
                );
                let fallback = owned_dialog.then(|| {
                    let mut view = shown.clone();
                    view.dialog = None;
                    view
                });
                menu::append_menu_nodes(
                    fallback.as_ref().unwrap_or(shown),
                    nodes,
                    next,
                    &mut self.layouts,
                    &self.font,
                    metrics,
                    self.solid_texture_page,
                    width,
                    height,
                    self.safe_area,
                    &mut self.menu_scrolls,
                )
                .map(|hits| {
                    // The programmatic fallback has no trust or join popup, so vanilla's draws over it.
                    if shown.server_trust_prompt().is_none()
                        && shown.join_request_prompt().is_none()
                        && !owned_dialog
                    {
                        return hits;
                    }
                    let state = ViewState::default();
                    let popup = self.append_dialog(
                        runtime,
                        shown,
                        &state,
                        nodes,
                        next,
                        metrics,
                        [width, height],
                    );
                    let (hits, keys) = popup.unwrap_or((hits, Vec::new()));
                    self.form_presentation.menu_keys = keys;
                    hits
                })
            }
        };
        if shown.popup_open() {
            self.menu_preview.control = None;
            self.menu_preview.revoke_capture();
        }
        if result.is_ok() && shown.visible {
            self.form_presentation.menu_focus_context = Some((shown.screen, shown.popup_open()));
        }
        self.menu_view = Some(view);
        result
    }

    /// `Ok(None)` when the engine has no screen for this state.
    #[allow(clippy::too_many_arguments)]
    fn append_engine_menu(
        &mut self,
        runtime: &UiRuntime,
        view: &MenuView,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        width: f32,
        height: f32,
    ) -> Result<Option<Vec<(MenuAction, UiRect)>>, UiPresentationError> {
        // Without the UI carrier the programmatic launcher draws every screen.
        if self.form_presentation.engine.is_none()
            && !matches!(
                view.screen,
                crate::menu::MenuScreen::DressingRoom | crate::menu::MenuScreen::Death
            )
        {
            return Ok(None);
        }
        // OreUI routes draw through the native canvas.
        let portrait = [
            &view.feeds.profile.picture_path,
            &view.feeds.home.persona_head,
        ]
        .into_iter()
        .find_map(|path| self.menu_artwork.refs.get(path).copied());
        if let Some(mut hits) = self.append_oreui_screen(
            view,
            nodes,
            next,
            metrics,
            [width, height],
            portrait,
            &|key| runtime.translation(key),
        )? {
            let mut keys = Vec::new();
            let popup = self.append_dialog(
                runtime,
                view,
                &ViewState::default(),
                nodes,
                next,
                metrics,
                [width, height],
            );
            if let Some(popup) = popup {
                (hits, keys) = popup;
                self.form_presentation.oreui_slider_tracks.clear();
                self.form_presentation.oreui_settings_input = false;
                self.form_presentation.menu_focus_geometry.clear();
                self.form_presentation.menu_focus_landmarks.clear();
            }
            if view.dialog.is_some() {
                self.form_presentation.menu_focus.clear();
                self.form_presentation.menu_focus_geometry.clear();
                self.form_presentation.menu_focus_landmarks.clear();
                self.form_presentation.oreui_slider_tracks.clear();
                self.form_presentation.oreui_settings_input = false;
            }
            self.form_presentation.menu_keys = keys;
            return Ok(Some(hits));
        }
        let Some(renderer) = self.form_presentation.engine.as_deref() else {
            return Ok(None);
        };
        let translate = |key: &str| runtime.translation(key);
        let Some(screen) = menu_screens::screen_data(view, &translate) else {
            return Ok(None);
        };
        // Last frame's region keys carry the launcher's hover/press/focus.
        let key_of = |action: Option<MenuAction>| {
            let action = action?;
            self.form_presentation
                .menu_keys
                .iter()
                .find(|(candidate, _)| *candidate == action)
                .map(|(_, key)| key.clone())
        };
        let scroll = self
            .menu_scrolls
            .offsets()
            .iter()
            .map(|(key, offset)| (key.clone(), f64::from(*offset)))
            .collect();
        let state = ViewState {
            scroll,
            hovered: key_of(view.hovered).or_else(|| key_of(view.focused_action)),
            pressed: key_of(view.pressed),
            focused: view.field.and_then(|field| {
                key_of(Some(match field {
                    crate::menu::MenuField::Name => MenuAction::AddName,
                    crate::menu::MenuField::Address => MenuAction::AddAddress,
                    crate::menu::MenuField::Port => MenuAction::AddPort,
                    // Drawn by the OreUI create and edit screens, not JSON-UI.
                    crate::menu::MenuField::WorldName
                    | crate::menu::MenuField::WorldSeed
                    | crate::menu::MenuField::SkinName
                    | crate::menu::MenuField::RealmCode => {
                        return None;
                    }
                }))
            }),
            ..ViewState::default()
        };
        let edit = engine::host_edit::Feedback::from_view(view);
        let rollback = (nodes.len(), *next);
        // A popup draws over its screen and alone takes the input, so only the last frame's regions count.
        let mut layers = Vec::new();
        let mut layer = Some(&screen);
        while let Some(current) = layer {
            layers.push(current);
            layer = current.overlay.as_deref();
        }
        let mut drawn = None;
        let preview_view = std::cell::Cell::new(None);
        let preview_control = std::cell::Cell::new(None);
        let top = layers.len() - 1;
        for (index, layer) in layers.into_iter().enumerate() {
            if !renderer
                .scene_settings(layer.reference, &layer.context)
                .renders(
                    index == top
                        && (!view.popup_open()
                            || view.dialog == Some(crate::menu::MenuDialog::DeathQuit)),
                )
            {
                continue;
            }
            let inputs = engine::EngineInputs {
                layouts: &mut self.layouts,
                font: &self.font,
                metrics,
                solid_page: self.solid_texture_page,
                safe_area: self.safe_area,
                content: [width, height],
                translate: &translate,
                language: runtime.text_generation(),
            };
            let out = engine::EngineOutput {
                nodes: &mut *nodes,
                next: &mut *next,
                overlay: &[],
            };
            let art = engine::ScreenArt {
                icons: &[],
                edit,
                preview: self.hud_frame.player_preview,
                preview_view: Some(&preview_view),
                preview_control: Some(&preview_control),
                preview_rotation: self.menu_preview.rotation(),
                pointer: self.menu_preview.pointer.map(|point| {
                    let gui_pixel =
                        metrics.scale.get() * super::super::FONT_DESIGN_PIXEL_TEXELS as f32;
                    [
                        (point.x() - self.safe_area.left()) / gui_pixel,
                        (point.y() - self.safe_area.top()) / gui_pixel,
                    ]
                }),
                images: Some(&self.menu_artwork.refs),
                // The gamerpic, else the rendered persona head.
                portrait: [
                    super::accounts::current_picture(view).unwrap_or_default(),
                    &view.feeds.home.persona_head,
                ]
                .into_iter()
                .find_map(|path| self.menu_artwork.refs.get(path).copied()),
                splash: renderer.splash(&translate),
                now: self.menu_seconds,
                clocks: Some(&self.scene_clock),
                ..engine::ScreenArt::default()
            };
            match renderer.render_screen(
                layer.reference,
                &layer.data,
                &layer.context,
                &state,
                art,
                inputs,
                out,
            ) {
                Ok(Some(frame)) => drawn = Some(frame),
                Ok(None) | Err(_) => {
                    nodes.truncate(rollback.0);
                    *next = rollback.1;
                    return Ok(None);
                }
            }
        }
        if let Some(view) = preview_view.get() {
            self.player_preview_view = view;
        }
        let Some(frame) = drawn else {
            if let Some((hits, keys)) =
                self.append_dialog(runtime, view, &state, nodes, next, metrics, [width, height])
            {
                self.form_presentation.menu_keys = keys;
                return Ok(Some(hits));
            }
            return Ok(None);
        };
        if !view.popup_open()
            && let Some(mut control) = preview_control.get()
        {
            let origin = [self.safe_area.left(), self.safe_area.top()];
            control.bounds = rect(
                control.bounds.min().x() + origin[0],
                control.bounds.min().y() + origin[1],
                control.bounds.max().x() + origin[0],
                control.bounds.max().y() + origin[1],
            )?;
            if let Some(bounds) = frame.hits.iter().rev().find_map(|region| {
                (region.enabled && region.widget.gesture.as_deref() == Some("button.turn_doll"))
                    .then(|| window_rect(region, frame.scale, origin))
                    .flatten()
            }) {
                control.bounds = bounds;
            }
            self.menu_preview.control = Some(control);
        }
        let mut hits = Vec::new();
        let mut keys = Vec::new();
        for region in json_ui::focus_order(&frame.hits) {
            let actions = super::global_resources::slider_actions(view, region)
                .or_else(|| menu_screens::slider_actions(view, region))
                .unwrap_or_else(|| menu_screens::action_for(view, region).into_iter().collect());
            self.form_presentation
                .menu_focus
                .extend(actions.iter().copied());
            for action in actions {
                keys.push((action, region.key.clone()));
            }
        }
        let focused_key = keys.iter().find_map(|(action, key)| {
            (Some(*action) == view.focused_action).then_some(key.as_str())
        });
        self.menu_scrolls
            .reveal_engine_focus(view.focused_action, focused_key, &frame);
        let mut sounds = Vec::new();
        let mut spots = Vec::new();
        let origin = [self.safe_area.left(), self.safe_area.top()];
        self.menu_scrolls.set_areas(scroll_areas(&frame, origin));
        for region in frame.hits.iter().filter(|region| region.enabled) {
            if let Some(actions) = super::global_resources::slider_actions(view, region)
                .or_else(|| menu_screens::slider_actions(view, region))
            {
                let mut track = region.clone();
                track.clip = track.rect;
                self.settings_slider_drag_targets.extend(
                    segments(&track, actions.len(), frame.scale, origin)
                        .into_iter()
                        .map(|(step, bounds)| (actions[step], bounds)),
                );
                for (step, bounds) in segments(region, actions.len(), frame.scale, origin) {
                    hits.push((actions[step], bounds));
                    if !region.takes_focus() {
                        keys.push((actions[step], region.key.clone()));
                    }
                }
                continue;
            }
            let Some(action) = menu_screens::action_for(view, region) else {
                continue;
            };
            if let Some(bounds) = window_rect(region, frame.scale, origin) {
                hits.push((action, bounds));
                if !keys.iter().any(|(candidate, _)| *candidate == action) {
                    keys.push((action, region.key.clone()));
                }
                sounds.extend(region.sound.clone().map(|sound| (action, sound)));
                spots.extend(text_spot(&frame, region, action, bounds, metrics));
            }
        }
        self.add_menu_text_spots(spots);
        self.form_presentation.menu_sounds = sounds;
        // An owned dialog or the join's trust question takes all input over its screen.
        if let Some(popup) =
            self.append_dialog(runtime, view, &state, nodes, next, metrics, [width, height])
        {
            (hits, keys) = popup;
            self.form_presentation.menu_focus = hits.iter().map(|(action, _)| *action).collect();
        }
        self.form_presentation.menu_keys = keys;
        Ok(Some(hits))
    }
}

impl UiPresentationRuntime {
    /// The vanilla popup for the join's trust question, else `view`'s open dialog, else a Discord
    /// join request, drawn over its screen with the only hit targets that then count.
    #[allow(clippy::too_many_arguments)]
    fn append_dialog(
        &mut self,
        runtime: &UiRuntime,
        view: &MenuView,
        state: &ViewState,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        [width, height]: [f32; 2],
    ) -> Option<MenuHits> {
        let trust = view.server_trust_prompt();
        let join = view.join_request_prompt();
        let dialog = if trust.is_some() || join.is_some() {
            None
        } else {
            Some(view.dialog?)
        };
        if matches!(
            dialog,
            Some(crate::menu::MenuDialog::Accounts | crate::menu::MenuDialog::Exit)
        ) {
            self.form_presentation.menu_sounds = Vec::new();
            let rollback = (nodes.len(), *next);
            let drawn = if dialog == Some(crate::menu::MenuDialog::Exit) {
                self.append_oreui_exit(view, nodes, next, metrics, [width, height], &|key| {
                    runtime.translation(key)
                })
            } else {
                self.append_oreui_accounts(view, nodes, next, metrics, [width, height])
            };
            return Some(match drawn {
                Ok(hits) => (hits, Vec::new()),
                Err(_) => {
                    nodes.truncate(rollback.0);
                    *next = rollback.1;
                    (Vec::new(), Vec::new())
                }
            });
        }
        let Some(renderer) = self.form_presentation.engine.as_deref() else {
            return Some((Vec::new(), Vec::new()));
        };
        let rollback = (nodes.len(), *next);
        let translate = |key: &str| runtime.translation(key);
        let (model, confirm, dismiss) = match (trust, join, dialog) {
            (Some(prompt), _, _) => (
                menu_screens::server_trust_model(&prompt.url, &translate),
                MenuAction::ServerTrust(true),
                MenuAction::ServerTrust(false),
            ),
            (None, Some(name), _) => (
                menu_screens::join_request_model(name, &translate),
                MenuAction::JoinRequest(true),
                MenuAction::JoinRequest(false),
            ),
            (None, None, dialog) => {
                let (model, confirm) = menu_screens::dialog_model(view, dialog?, &translate);
                (model, confirm, MenuAction::DismissDialog)
            }
        };
        let context = json_ui::form_context(&model, &menu_screens::retail_context());
        let mut data = json_ui::form_data_source(&model);
        let reference = if dialog
            == Some(crate::menu::MenuDialog::SettingsSupport(
                crate::menu::settings_support::SupportDialog::Help,
            )) {
            super::settings_support::help_data(&mut data, &translate);
            "rating_prompt.rating_prompt_screen"
        } else {
            MODAL_POPUP
        };
        let inputs = engine::EngineInputs {
            layouts: &mut self.layouts,
            font: &self.font,
            metrics,
            solid_page: self.solid_texture_page,
            safe_area: self.safe_area,
            content: [width, height],
            translate: &translate,
            language: runtime.text_generation(),
        };
        let out = engine::EngineOutput {
            nodes: &mut *nodes,
            next: &mut *next,
            overlay: &[],
        };
        let popup = renderer.render_screen(
            reference,
            &data,
            &context,
            state,
            engine::ScreenArt {
                now: self.menu_seconds,
                images: Some(&self.menu_artwork.refs),
                ..engine::ScreenArt::default()
            },
            inputs,
            out,
        );
        let popup = match popup {
            Ok(Some(popup)) => popup,
            _ => {
                nodes.truncate(rollback.0);
                *next = rollback.1;
                self.menu_scrolls.set_areas(Vec::new());
                return Some((Vec::new(), Vec::new()));
            }
        };
        let origin = [self.safe_area.left(), self.safe_area.top()];
        self.menu_scrolls.set_areas(scroll_areas(&popup, origin));
        let mut hits = Vec::new();
        let mut keys = Vec::new();
        let mut sounds = Vec::new();
        for region in popup.hits.iter().filter(|region| region.enabled) {
            let action = match region.pressed.as_deref() {
                Some("popup_dialog.left_button" | "button.rating_yes_button") => confirm,
                Some(
                    "popup_dialog.rightcancel_button"
                    | "popup_dialog.escape"
                    | "button.menu_exit"
                    | "button.rating_no_button",
                ) => dismiss,
                _ => continue,
            };
            if let Some(bounds) = window_rect(region, popup.scale, origin) {
                hits.push((action, bounds));
                keys.push((action, region.key.clone()));
                sounds.extend(region.sound.clone().map(|sound| (action, sound)));
            }
        }
        self.form_presentation.menu_sounds = sounds;
        Some((hits, keys))
    }
}

/// A region's clipped rect in window-logical pixels.
pub(super) fn window_rect(region: &HitRegion, scale: f32, origin: [f32; 2]) -> Option<UiRect> {
    let x0 = region.rect.x.max(region.clip.x);
    let y0 = region.rect.y.max(region.clip.y);
    let x1 = (region.rect.x + region.rect.w).min(region.clip.x + region.clip.w);
    let y1 = (region.rect.y + region.rect.h).min(region.clip.y + region.clip.h);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let to = |value: f64, axis: usize| value as f32 * scale + origin[axis];
    rect(to(x0, 0), to(y0, 1), to(x1, 0), to(y1, 1)).ok()
}

/// Where a launcher text box's region drew its text, for placing a pressed caret.
fn text_spot(
    frame: &EngineFrame,
    region: &HitRegion,
    action: MenuAction,
    bounds: UiRect,
    metrics: TextMetrics,
) -> Option<TextSpot> {
    let field = action.text_field()?;
    let text = frame
        .edit_texts
        .iter()
        .find(|text| text.key == region.key)?;
    Some(TextSpot {
        field,
        bounds,
        left: text.left,
        factor: text.scale,
        font: text.font.clone(),
        letter_spacing_64: 0,
        metrics,
    })
}

/// The frame's scroll views in window-logical pixels, offsets in virtual px.
fn scroll_areas(frame: &EngineFrame, origin: [f32; 2]) -> Vec<ScrollArea> {
    let window = |r: [f64; 4]| {
        let to = |value: f64, axis: usize| value as f32 * frame.scale + origin[axis];
        rect(
            to(r[0], 0),
            to(r[1], 1),
            to(r[0] + r[2], 0),
            to(r[1] + r[3], 1),
        )
        .ok()
    };
    frame
        .hits
        .iter()
        .filter(|region| region.kind == json_ui::HitKind::ScrollView)
        .filter_map(|region| {
            let metrics = frame.report.scrolls.get(&region.key)?;
            Some(ScrollArea {
                key: region.key.clone(),
                viewport: metrics
                    .viewport_rect
                    .and_then(window)
                    .or_else(|| window_rect(region, frame.scale, origin))?,
                scale: frame.scale,
                offset: metrics.offset as f32,
                max: metrics.max_offset() as f32,
                speed: metrics.speed as f32,
                track: metrics.track.and_then(window),
                thumb: metrics.thumb.and_then(window),
                engine: Some((metrics.clone(), origin)),
                draggable: metrics.box_drag != json_ui::Draggable::NotDraggable,
            })
        })
        .collect()
}

/// Regions select the nearest slider anchor, including anchors at both ends
/// of the track. The end values therefore occupy half an interior interval.
pub(super) fn segments(
    region: &HitRegion,
    steps: usize,
    scale: f32,
    origin: [f32; 2],
) -> Vec<(usize, UiRect)> {
    let interval = region.rect.w / steps.saturating_sub(1).max(1) as f64;
    (0..steps)
        .filter_map(|step| {
            let mut part = region.clone();
            let left = if step == 0 {
                0.0
            } else {
                (step as f64 - 0.5) * interval
            };
            let right = if step + 1 == steps {
                region.rect.w
            } else {
                (step as f64 + 0.5) * interval
            };
            part.rect.x = region.rect.x + left;
            part.rect.w = right - left;
            window_rect(&part, scale, origin).map(|bounds| (step, bounds))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use json_ui::{HitKind, RectOut};
    use ui::UiPoint;

    use super::*;

    #[test]
    fn gui_scale_slider_regions_choose_the_nearest_native_step() {
        let rect = RectOut {
            x: 10.0,
            y: 20.0,
            w: 100.0,
            h: 12.0,
        };
        let region = HitRegion {
            key: "gui_scale".into(),
            name: "gui_scale".into(),
            kind: HitKind::Slider,
            rect,
            clip: rect,
            layer: 0,
            order: 0,
            pressed: None,
            control_name: Some("gui_scale".into()),
            collection_index: None,
            collection: None,
            enabled: true,
            checked: None,
            max_length: None,
            group_index: None,
            renderer: None,
            drag_axes: [false; 2],
            sound: None,
            input: Default::default(),
            focus: None,
            collections: Vec::new(),
            widget: Default::default(),
            modal_root: None,
        };
        let hits = segments(&region, 3, 2.0, [5.0, 7.0]);
        for (track_x, expected) in [
            (0.0, 0),
            (24.0, 0),
            (25.0, 1),
            (30.0, 1),
            (70.0, 1),
            (75.0, 2),
            (99.0, 2),
        ] {
            let point = UiPoint::new(5.0 + 2.0 * (10.0 + track_x), 7.0 + 2.0 * 26.0).unwrap();
            let selected = hits
                .iter()
                .rev()
                .find(|(_, bounds)| bounds.contains(point))
                .map(|(step, _)| *step);
            assert_eq!(selected, Some(expected), "track position {track_x}");
        }
    }
}
