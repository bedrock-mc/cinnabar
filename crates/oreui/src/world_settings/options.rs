//! Catalog-driven controls shared by world creation and editing.

use super::*;
use launcher::local_worlds::options::{Control, WorldOption as O};

/// Paints every available option in the selected tab and returns the content's bottom.
pub(super) fn draw(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    local_view: &WorldsView,
    area: Bounds,
    mut y: f32,
    editing: bool,
) -> Result<f32, PaintError> {
    if !local_view.options_available() {
        return Ok(y);
    }
    let options = local_view
        .edit
        .as_ref()
        .filter(|_| editing)
        .map_or(&local_view.create.options, |edit| &edit.options);
    if let Some(error) = local_view.form_error {
        y = caption(canvas, error, area, y)? + space(canvas, 1);
    }
    for &id in O::ALL {
        let definition = id.definition();
        if definition.tab != local_view.tab || (editing && definition.creation_only) {
            continue;
        }
        y = sections::row(canvas, area, y, |canvas, inner| {
            let enabled = !local_view.busy && local_view.option_editable(options, id, editing);
            match definition.control {
                Control::Toggle => {
                    let width = canvas.r(7.6);
                    let end = label(
                        canvas,
                        definition.label,
                        [inner[0], inner[1], inner[2] - width, inner[3]],
                        inner[1],
                    )?;
                    let action = local(A::Option(id, 1 - options.get(id)));
                    let height = canvas.r(3.2);
                    let bottom = end.max(inner[1] + height);
                    super::super::settings::controls::toggle_control(
                        canvas,
                        view,
                        [inner[2] - canvas.r(6.4), (inner[1] + bottom - height) * 0.5],
                        local_view.option_value(options, id) != 0,
                        action,
                        enabled,
                    )?;
                    if enabled {
                        canvas.hit(action, [inner[0], inner[1], inner[2], bottom])?;
                    }
                    if id == O::ActivateCheats {
                        caption(
                            canvas,
                            "Changing a cheat setting activates cheats. Achievements stay disabled once cheats are used.",
                            inner,
                            bottom + space(canvas, 1),
                        )
                    } else {
                        Ok(bottom)
                    }
                }
                Control::Choice(choices) => {
                    let y = label(canvas, definition.label, inner, inner[1])?;
                    let choices: Vec<_> = choices
                        .iter()
                        .map(|choice| {
                            (
                                choice.label,
                                local(A::Option(id, choice.value)),
                                options.get(id) == choice.value,
                            )
                        })
                        .collect();
                    let height = choice_height(canvas, inner, &choices)?;
                    segmented(canvas, view, [inner[0], y, inner[2], y + height], &choices)?;
                    Ok(y + height)
                }
                Control::Number { min, max } => {
                    let y = label(canvas, definition.label, inner, inner[1])?;
                    let bounds = [inner[0], y, inner[2], y + canvas.r(FIELD)];
                    let value = local_view
                        .option_text
                        .get(&id)
                        .cloned()
                        .unwrap_or_else(|| options.get(id).to_string());
                    text_field_on_panel(
                        canvas,
                        view,
                        bounds,
                        &value,
                        "",
                        view.field == Some(MenuField::WorldOption(id)),
                        enabled.then_some(local(A::OptionField(id))),
                    )?;
                    let valid = value.parse().is_ok_and(|value| id.accepts(value));
                    let range = format!("{min}–{max}");
                    caption(
                        canvas,
                        &if valid {
                            range
                        } else {
                            format!("Enter a number in the range {range}")
                        },
                        inner,
                        bounds[3] + space(canvas, 1),
                    )
                }
            }
        })?;
    }
    Ok(y)
}
