//! Scripted editor captures use only host-owned pointer and keyboard routing.

use super::*;

/// Paints a selected card before editing, or after a move, anchor, scale and hide action.
pub(super) fn paint(
    engine: &FormEngine,
    frame: &mut PaintFrame<'_>,
    hud: &ui::mod_hud::Hud,
    edited: bool,
) -> Result<(), Error> {
    let mut ui = mod_ui::ModUi::default();
    let panel: ui::mod_panel::Panel = serde_json::from_value(serde_json::json!({"title":"HUD Layout","toggle_key":"ShiftRight","dark":true,"controls":[]}))
        .map_err(|error| Error::Invalid(error.to_string()))?;
    ui.set_mod_panel(mod_ui::PanelUiOwner::Environment(0), Some(&panel))?;
    ui.set_mod_panel_open(true);
    ui.open_mod_hud_editor(hud)?;
    draw(&mut ui, engine, frame);
    click(&mut ui, frame, [80., 110.]);
    draw(&mut ui, engine, frame);
    if edited {
        // Move first, then use the guest's Center, Scale and Hide buttons.
        pointer(&mut ui, frame, [80., 110.], true, true);
        pointer(&mut ui, frame, [180., 130.], false, true);
        pointer(&mut ui, frame, [180., 130.], false, false);
        draw(&mut ui, engine, frame);
        for x in [313., 375., 437.] {
            click(&mut ui, frame, [x, 24.]);
            draw(&mut ui, engine, frame);
        }
        frame.now += 0.35;
        draw(&mut ui, engine, frame);
    }
    if !ui.mod_hud_editor_open() {
        return Err(Error::Invalid(
            "HUD editor capture did not stay open".into(),
        ));
    }
    Ok(())
}

/// Replaces the capture's output with one production editor frame.
fn draw(ui: &mut mod_ui::ModUi, engine: &FormEngine, frame: &mut PaintFrame<'_>) {
    frame.nodes.clear();
    *frame.next = 1;
    let icons = ui.mod_hud_editor_icons(|_, _| None).unwrap();
    ui.paint_mod_hud_editor(Some(engine), frame, &icons);
}

/// Sends GUI coordinates through the same physical-scale mapping as the painter.
fn pointer(
    ui: &mut mod_ui::ModUi,
    frame: &PaintFrame<'_>,
    point: [f32; 2],
    pressed: bool,
    held: bool,
) {
    let px = frame.metrics.scale.get() * ui::FONT_DESIGN_PIXEL_TEXELS as f32;
    ui.mod_panel_events(point.map(|value| value * px), pressed, held);
}

/// Completes a single click without keeping pointer capture across actions.
fn click(ui: &mut mod_ui::ModUi, frame: &PaintFrame<'_>, point: [f32; 2]) {
    pointer(ui, frame, point, true, true);
    pointer(ui, frame, point, false, false);
}
