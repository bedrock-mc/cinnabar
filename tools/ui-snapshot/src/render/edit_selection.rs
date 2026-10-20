//! An anonymous custom form driven through production painting and component input.

use json_ui::{
    ButtonInput, CustomElement, CustomForm, Dispatcher, FormModel, InputMode, PointerInput,
    ViewState,
};
use ui_engine::{engine::FormEngine, engine::screen::PaintFrame, frame::EngineFrame};

use crate::carriers::Error;

/// Selects each input in order and keeps the caret's initial shown phase for the final paint.
pub(super) fn paint(engine: &mut FormEngine, frame: &mut PaintFrame<'_>) -> Result<(), Error> {
    let model = FormModel::Custom(CustomForm {
        title: "Selection transfer".into(),
        elements: ["First", "Second"]
            .map(|name| CustomElement::Input {
                enabled: true,
                text: format!("{name} input"),
                value: String::new(),
                placeholder: format!("{name} placeholder"),
                tooltip: String::new(),
            })
            .into(),
        submit_text: "Done".into(),
        submit_visible: true,
        ..Default::default()
    });
    let mut view = ViewState::default();
    let mut dispatcher = Dispatcher::default();
    let mut drawn = draw(engine, &model, &view, frame)?;
    let mut edits: Vec<_> = drawn
        .hits
        .iter()
        .filter(|region| region.enabled && region.widget.edit.is_some())
        .cloned()
        .collect();
    edits.sort_by(|a, b| a.rect.y.total_cmp(&b.rect.y));
    if edits.len() != 2 {
        return Err(Error::Invalid(format!(
            "selection fixture requires two visible edit boxes, found {}",
            edits.len()
        )));
    }
    for (index, edit) in edits.iter().enumerate() {
        let point = [
            edit.rect.x + edit.rect.w / 2.0,
            edit.rect.y + edit.rect.h / 2.0,
        ];
        let now = index as f64;
        dispatcher.pointer(
            &drawn.hits,
            &mut view,
            PointerInput {
                point: Some(point),
                held: false,
                mode: InputMode::Mouse,
                now,
            },
        );
        for (down, now) in [(true, now), (false, now + 0.05)] {
            dispatcher.button(
                &drawn.hits,
                &mut view,
                ButtonInput {
                    id: "button.menu_select",
                    down,
                    point: Some(point),
                    mode: InputMode::Mouse,
                    now,
                },
            );
            drawn = draw(engine, &model, &view, frame)?;
        }
        if view.components.selected() != Some(edit.key.as_str()) {
            return Err(Error::Invalid(format!(
                "selection fixture could not select input {}",
                index + 1
            )));
        }
    }
    Ok(())
}

/// Repaints the current component writes with no wall clock or caret-blink tick.
fn draw(
    engine: &mut FormEngine,
    model: &FormModel,
    view: &ViewState,
    frame: &mut PaintFrame<'_>,
) -> Result<EngineFrame, Error> {
    frame.nodes.clear();
    *frame.next = 1;
    frame.now = super::CAPTURE_SECONDS;
    engine
        .render(model, view, frame)
        .map_err(|error| Error::Invalid(error.to_string()))?
        .ok_or_else(|| Error::Invalid("selection fixture's custom form did not resolve".into()))
}
