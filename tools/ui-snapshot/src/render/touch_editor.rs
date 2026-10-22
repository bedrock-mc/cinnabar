//! Anonymous captures replay pointer gestures through the built-in touch editor.
use crate::{
    carriers::{self, Error},
    fixtures::State,
};
use client_ui::ui_runtime::{UiRuntime, presentation::UiPresentationRuntime};
use image::ImageEncoder;
use launcher::menu::{MenuScreen, MenuView};
use std::{path::Path, sync::Arc};

/// Paints default, aligned, saved and time-sampled editing states with real control artwork.
pub(super) fn png(state: &State, dir: &Path, phase: u8, seconds: f64) -> Result<Vec<u8>, Error> {
    let carriers = carriers::load(dir, true)?;
    let mut presentation =
        UiPresentationRuntime::new(carriers.font).map_err(|error| error.to_string())?;
    presentation.enable_json_ui(carriers.ui)?;
    if let Some(images) = oreui::images::load_optional_oreui_images(dir) {
        presentation.enable_oreui_originals(images)?;
    }
    presentation.set_gui_scale_preference(Some(state.gui_scale));
    if phase == 5 {
        presentation.set_safe_area(ui::SafeArea::new(24., 32., 24., 32.).unwrap());
    }
    let mut runtime = UiRuntime::new(1);
    runtime.set_lang_catalog(Arc::new(carriers.lang));
    let player = player_state::PlayerState::new(1);
    let dpi = ui::DpiScale::new(if phase >= 4 { 2. } else { 1. }).unwrap();
    let mut view = MenuView::new(true, "Player".into());
    view.screen = MenuScreen::Settings;
    view.touch_available = true;
    view.touch_editor_open = true;
    presentation.set_menu_view(Some(view.clone()));
    let draw = |p: &mut UiPresentationRuntime| {
        p.build(&player, &runtime, 1000, state.size, dpi)
            .map_err(|e| Error::Invalid(e.to_string()))
    };
    draw(&mut presentation)?;
    if phase >= 4 {
        if phase == 5 {
            for action in ["save", "cancel", "undo", "redo", "reset", "grid", "snap"] {
                while presentation
                    .touch_editor_action_bounds(&format!("hud.{action}"))
                    .is_none()
                {
                    let [x, y, w, h] = presentation
                        .touch_editor_action_bounds("hud.toolbar_next")
                        .ok_or_else(|| Error::Invalid("compact action pager missing".into()))?;
                    if presentation
                        .touch_editor_action_at([x + w / 2., y + h / 2.])
                        .as_deref()
                        != Some("hud.toolbar_next")
                    {
                        return Err(Error::Invalid("compact action is unreachable".into()));
                    }
                    presentation.touch_editor_pointer([x + w / 2., y + h / 2.], true, false);
                    draw(&mut presentation)?;
                }
            }
            while presentation
                .touch_editor_action_bounds("hud.save")
                .is_none()
            {
                let [x, y, w, h] = presentation
                    .touch_editor_action_bounds("hud.toolbar_prev")
                    .ok_or_else(|| Error::Invalid("compact return action missing".into()))?;
                if presentation
                    .touch_editor_action_at([x + w / 2., y + h / 2.])
                    .as_deref()
                    != Some("hud.toolbar_prev")
                {
                    return Err(Error::Invalid(
                        "compact return action is unreachable".into(),
                    ));
                }
                presentation.touch_editor_pointer([x + w / 2., y + h / 2.], true, false);
                draw(&mut presentation)?;
            }
            if presentation.take_touch_editor_result().is_some() {
                return Err(Error::Invalid(
                    "paging unexpectedly closed the draft".into(),
                ));
            }
        }
        // Reach the final compact page through the same actions used by a finger.
        while presentation.touch_editor_handle_bounds("menu").is_none() {
            let [x, y, w, h] = presentation
                .touch_editor_action_bounds("hud.page_next")
                .ok_or_else(|| Error::Invalid("compact page action missing".into()))?;
            if presentation
                .touch_editor_action_at([x + w / 2., y + h / 2.])
                .as_deref()
                != Some("hud.page_next")
            {
                return Err(Error::Invalid("compact page action is unreachable".into()));
            }
            presentation.touch_editor_pointer([x + w / 2., y + h / 2.], true, false);
            draw(&mut presentation)?;
        }
        let [x, y, w, h] = presentation
            .touch_editor_handle_bounds("menu")
            .ok_or_else(|| Error::Invalid("compact Pause handle missing".into()))?;
        if !presentation
            .touch_editor_action_at([x + w / 2., y + h / 2.])
            .is_some_and(|action| action.starts_with("hud.card:"))
        {
            return Err(Error::Invalid("compact Pause handle is obstructed".into()));
        }
    } else if phase != 0 {
        let [x, y, w, h] = presentation
            .touch_editor_action_bounds("hud.grid")
            .ok_or_else(|| Error::Invalid("Grid action missing".into()))?;
        presentation.touch_editor_pointer([x + w / 2., y + h / 2.], true, true);
        presentation.touch_editor_pointer([x + w / 2., y + h / 2.], false, false);
        draw(&mut presentation)?;
        let [x, y, w, h] = presentation
            .touch_editor_bounds("jump")
            .ok_or_else(|| Error::Invalid("touch editor did not open".into()))?;
        let start = [x + w / 2., y + h / 2.];
        let t = if phase == 3 {
            ((seconds - 1.5) / 2.).clamp(0., 1.) as f32
        } else {
            1.
        };
        let px = ui_engine::text_metrics::TextMetrics::for_viewport(
            state.size,
            dpi,
            Some(state.gui_scale),
        )
        .scale
        .get()
            * ui::FONT_DESIGN_PIXEL_TEXELS as f32;
        // Align Jump's left edge with the viewport center to show the shared guide.
        let end = [
            state.size[0] as f32 / 2. + w / 2.,
            state.size[1] as f32 * 0.42,
        ];
        let at = std::array::from_fn(|axis| start[axis] + (end[axis] - start[axis]) * t);
        if phase != 3 || seconds >= 1.5 {
            presentation.touch_editor_pointer(start, true, true);
            presentation.touch_editor_pointer(at, false, true);
        }
        draw(&mut presentation)?;
        if phase == 2 || phase == 3 && seconds >= 4.5 {
            presentation.touch_editor_pointer(at, false, false);
            // A second moved utility proves toolbar controls are independent.
            let [x, y, w, h] = presentation.touch_editor_bounds("chat").unwrap();
            presentation.touch_editor_pointer([x + w / 2., y + h / 2.], true, true);
            presentation.touch_editor_pointer(
                [x + w / 2. + 38. * px, y + h / 2. + 26. * px],
                false,
                true,
            );
            presentation.touch_editor_pointer(
                [x + w / 2. + 38. * px, y + h / 2. + 26. * px],
                false,
                false,
            );
            draw(&mut presentation)?;
            let [x, y, w, h] = presentation.touch_editor_action_bounds("hud.save").unwrap();
            presentation.touch_editor_pointer([x + w / 2., y + h / 2.], true, true);
            presentation.touch_editor_pointer([x + w / 2., y + h / 2.], false, false);
            let layout = presentation
                .take_touch_editor_result()
                .flatten()
                .ok_or_else(|| {
                    Error::Invalid("touch editor Save did not produce a layout".into())
                })?;
            if layout.position("chat").is_none() {
                return Err(Error::Invalid("Chat edit was not saved".into()));
            }
            let mut options = (*view.settings_options).clone();
            options.set_touch_layout(layout);
            presentation.set_chat_settings_snapshot((Arc::new(options), None));
            view.touch_editor_open = false;
            view.visible = false;
            presentation.set_menu_view(None);
            presentation.enable_touch_controls(true);
        }
    }
    let frame = draw(&mut presentation)?;
    let pixels = client_ui::test_support::rasterize(&frame);
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(
            &pixels,
            state.size[0],
            state.size[1],
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| Error::Invalid(e.to_string()))?;
    Ok(png)
}
