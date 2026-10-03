//! Menus through the engine: each menu state opens its vanilla screen, and the
//! screen's pressed regions become the launcher's own hit targets, so the menu
//! state machine and its input path stay unchanged. States without a vanilla
//! screen, or a render that fails, fall back to the programmatic launcher.

use json_ui::{HitRegion, ViewState};
use ui::{UiNode, UiRect};

use super::super::menu_scroll::ScrollArea;
use super::super::{TextMetrics, UiPresentationError, UiPresentationRuntime, menu, rect};
use super::{engine, menu_caret::TextSpot, menu_screens};
use crate::menu::{MenuAction, MenuScreen, MenuView};
use crate::ui_runtime::{UiRuntime, forms::EngineFrame};

const MODAL_POPUP: &str = "popup_dialog.modal_dialog_popup";

/// A menu frame's hit targets and, for hover next frame, their region keys.
type MenuHits = (Vec<(MenuAction, UiRect)>, Vec<(MenuAction, String)>);

impl UiPresentationRuntime {
    /// Draw the visible menu and return its window-logical hit targets.
    pub(crate) fn append_menu(
        &mut self,
        runtime: &UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        width: f32,
        height: f32,
    ) -> Result<Vec<(MenuAction, UiRect)>, UiPresentationError> {
        self.gui_scale_drag_targets.clear();
        let Some(mut view) = self.menu_view.take() else {
            return Ok(Vec::new());
        };
        self.begin_menu_caret(&mut view);
        let previous = self.form_presentation.ready_menu.take();
        let pending = view.screen == MenuScreen::Settings
            && previous
                .as_ref()
                .is_some_and(|view| matches!(view.screen, MenuScreen::Home | MenuScreen::Pause))
            && !self.prepare_settings(runtime, &view, metrics, [width, height]);
        let shown = if pending {
            previous.as_ref().unwrap()
        } else {
            &view
        };
        self.menu_scrolls.begin_frame(format!(
            "{:?}/{:?}/{:?}/{}",
            shown.screen, shown.server_tab, shown.profile_tab, shown.settings_section
        ));
        self.menu_scrolls.set_areas(Vec::new());
        let drawn = if shown.visible {
            self.append_engine_menu(runtime, shown, nodes, next, metrics, width, height)
        } else {
            Ok(Some(Vec::new()))
        };
        let result = match drawn {
            Ok(Some(hits)) => Ok(hits),
            Ok(None) | Err(_) => menu::append_menu_nodes(
                shown,
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
            ),
        };
        self.form_presentation.ready_menu = if pending
            || previous
                .as_ref()
                .is_some_and(|previous| previous.screen == view.screen)
        {
            previous
        } else {
            Some(view.clone())
        };
        self.menu_view = Some(view);
        if pending {
            self.form_presentation.menu_keys.clear();
            self.menu_scrolls.set_areas(Vec::new());
            result.map(|_| Vec::new())
        } else {
            result
        }
    }

    /// Prepare Settings without blocking its opening frame; readiness includes all layout inputs.
    fn prepare_settings(
        &self,
        runtime: &UiRuntime,
        view: &MenuView,
        metrics: TextMetrics,
        [width, height]: [f32; 2],
    ) -> bool {
        let Some(renderer) = self.form_presentation.engine.as_deref() else {
            return true;
        };
        let translate = |key: &str| runtime.translation(key);
        let Some(prepared) = settings_preparation(view, &translate) else {
            return true;
        };
        let px = metrics.scale.get() * super::super::FONT_DESIGN_PIXEL_TEXELS as f32;
        renderer.prepare(engine::screen_cache::Prepared {
            reference: prepared.reference,
            context: prepared.context,
            data: prepared.data,
            root: [f64::from(width / px), f64::from(height / px)],
            px,
            language: runtime.text_generation(),
            font: std::sync::Arc::clone(&self.font),
            metrics,
            translator: runtime.translator(),
        })
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
        if self.form_presentation.engine.is_none() {
            return Ok(None);
        }
        // Screens 26.30 draws with OreUI by default draw natively.
        let portrait = [
            &view.feeds.profile.picture_path,
            &view.feeds.home.persona_head,
        ]
        .into_iter()
        .find_map(|path| self.menu_artwork.refs.get(path).copied());
        if let Some(hits) =
            self.append_oreui_screen(view, nodes, next, metrics, [width, height], portrait)?
        {
            let popup = self.append_dialog(
                runtime,
                view,
                &ViewState::default(),
                nodes,
                next,
                metrics,
                [width, height],
            );
            let (hits, keys) = popup.unwrap_or((hits, Vec::new()));
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
        if matches!(view.screen, MenuScreen::Home | MenuScreen::Pause) {
            self.prepare_settings(runtime, view, metrics, [width, height]);
        }
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
                    crate::menu::MenuField::WorldName | crate::menu::MenuField::WorldSeed => {
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
        let top = layers.len() - 1;
        for (index, layer) in layers.into_iter().enumerate() {
            if !renderer
                .scene_settings(layer.reference, &layer.context)
                .renders(index == top && view.dialog.is_none())
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
                pointer: None,
                images: Some(&self.menu_artwork.refs),
                // The gamerpic, else the rendered persona head.
                portrait: [
                    &view.feeds.profile.picture_path,
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
        let mut hits = Vec::new();
        let mut keys = Vec::new();
        let mut sounds = Vec::new();
        let mut spots = Vec::new();
        let origin = [self.safe_area.left(), self.safe_area.top()];
        self.menu_scrolls.set_areas(scroll_areas(&frame, origin));
        for region in frame.hits.iter().filter(|region| region.enabled) {
            if let Some(actions) = super::global_resources::slider_actions(view, region)
                .or_else(|| menu_screens::slider_actions(view, region))
            {
                if region.control_name.as_deref() == Some("gui_scale") {
                    let mut track = region.clone();
                    track.clip = track.rect;
                    self.gui_scale_drag_targets.extend(
                        segments(&track, actions.len(), frame.scale, origin)
                            .into_iter()
                            .map(|(step, bounds)| (actions[step], bounds)),
                    );
                }
                for (step, bounds) in segments(region, actions.len(), frame.scale, origin) {
                    hits.push((actions[step], bounds));
                    keys.push((actions[step], region.key.clone()));
                }
                continue;
            }
            let Some(action) = menu_screens::action_for(view, region) else {
                continue;
            };
            if let Some(bounds) = window_rect(region, frame.scale, origin) {
                hits.push((action, bounds));
                keys.push((action, region.key.clone()));
                sounds.extend(region.sound.clone().map(|sound| (action, sound)));
                spots.extend(text_spot(&frame, region, action, bounds, metrics));
            }
        }
        self.add_menu_text_spots(spots);
        self.form_presentation.menu_sounds = sounds;
        // A launcher dialog opens the vanilla popup and takes over the input.
        if let Some(popup) =
            self.append_dialog(runtime, view, &state, nodes, next, metrics, [width, height])
        {
            (hits, keys) = popup;
        }
        self.form_presentation.menu_keys = keys;
        Ok(Some(hits))
    }
}

impl UiPresentationRuntime {
    /// The vanilla popup for `view`'s open dialog, drawn over its screen, with
    /// the only hit targets that then count.
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
        let dialog = view.dialog?;
        let Some(renderer) = self.form_presentation.engine.as_deref() else {
            return Some((Vec::new(), Vec::new()));
        };
        let rollback = (nodes.len(), *next);
        let translate = |key: &str| runtime.translation(key);
        let (model, confirm) = menu_screens::dialog_model(view, dialog, &translate);
        let context = json_ui::form_context(&model, &menu_screens::retail_context());
        let mut data = json_ui::form_data_source(&model);
        let reference = if dialog
            == crate::menu::MenuDialog::SettingsSupport(
                crate::menu::settings_support::SupportDialog::Help,
            ) {
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
                ) => MenuAction::DismissDialog,
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

/// Prepare only Settings; transient progress and error screens need live artwork.
fn settings_preparation(
    view: &MenuView,
    translate: menu_screens::Translate<'_>,
) -> Option<menu_screens::MenuScreenData> {
    let mut settings = view.clone();
    settings.screen = MenuScreen::Settings;
    menu_screens::screen_data(&settings, translate).filter(|screen| {
        Some(screen.reference) == menu_screens::menu_reference(MenuScreen::Settings)
    })
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

#[cfg(test)]
mod preparation_tests {
    use super::*;

    #[test]
    fn progress_and_disconnect_views_never_prepare_without_artwork() {
        let home = crate::menu::MenuRuntime::new(true, 2, "Steve".into()).view();
        assert!(settings_preparation(&home, &|_| None).is_some());
        let mut connecting = home.clone();
        connecting.connecting = true;
        let mut local = home.clone();
        local.local.progress = Some(crate::local_worlds::Progress::connecting("Home"));
        let mut disconnected = home;
        disconnected.disconnect_message = Some("Disconnected".into());
        for view in [connecting, local, disconnected] {
            assert!(settings_preparation(&view, &|_| None).is_none());
        }
    }
}
