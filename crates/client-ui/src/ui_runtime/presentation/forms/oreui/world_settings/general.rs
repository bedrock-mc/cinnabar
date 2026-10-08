//! General settings retain the vanilla form rows and expose local hosting choices.

use super::*;

/// Draws the editable first page of world creation.
pub(super) fn create(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    local_view: &WorldsView,
    area: Bounds,
) -> Result<f32, UiPresentationError> {
    let form = &local_view.create;
    let y = sections::row(canvas, area, area[1], |canvas, inner| {
        name_field(canvas, view, &form.name, inner, inner[1])
    })?;
    let y = sections::row(canvas, area, y, |canvas, inner| {
        game_modes(
            canvas,
            view,
            &[GameMode::Survival, GameMode::Creative],
            form.game_mode,
            inner,
            inner[1],
        )
    })?;
    let y = sections::row(canvas, area, y, |canvas, inner| {
        difficulties(canvas, view, form.difficulty, inner, inner[1])
    })?;
    let y = sections::row(canvas, area, y, sections::hardcore)?;
    let y = advanced::terrain(canvas, view, local_view, [area[0], y, area[2], area[3]])?;
    let y = sections::row(canvas, area, y, |canvas, inner| {
        sections::cheats(canvas, view, inner, form.allow_cheats)
    })?;
    advanced::server(canvas, view, local_view, [area[0], y, area[2], area[3]])
}

/// Draws saved settings and the fixed terrain choice.
pub(super) fn edit(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    local_view: &WorldsView,
    area: Bounds,
) -> Result<f32, UiPresentationError> {
    let Some(edit) = &local_view.edit else {
        return Ok(area[1]);
    };
    let y = sections::row(canvas, area, area[1], |canvas, inner| {
        name_field(canvas, view, &edit.name, inner, inner[1])
    })?;
    let y = sections::row(canvas, area, y, |canvas, area| {
        game_modes(
            canvas,
            view,
            &[GameMode::Survival, GameMode::Creative, GameMode::Adventure],
            edit.game_mode,
            area,
            area[1],
        )
    })?;
    let y = sections::row(canvas, area, y, |canvas, inner| {
        difficulties(canvas, view, edit.difficulty, inner, inner[1])
    })?;
    sections::row(canvas, area, y, |canvas, area| {
        let y = area[1];
        let y = match &local_view.edited {
            // Fixed when the world was created.
            Some(world) => {
                let y = label(canvas, "World type", area, y)?;
                let height = canvas.text(
                    world_type_label(world.generator),
                    [area[0], y],
                    area[2] - area[0],
                    BODY,
                    TEXT_DIMMER,
                    false,
                )?;
                y + height + space(canvas, 4)
            }
            None => y,
        };
        let y = label(canvas, "File management", area, y)?;
        let half = (area[2] - area[0] - space(canvas, 2)) * 0.5;
        button(
            canvas,
            view,
            [area[0], y, area[0] + half, y + canvas.r(CONTROL)],
            Variant::Destructive,
            "Delete world",
            Some(local(A::Delete)),
        )?;
        if let Some(world) = &local_view.edited {
            let details = format!(
                "Size: {} - Last saved: {}",
                crate::menu::file_size(world.size_bytes),
                crate::menu::civil_date(world.last_played_unix.max(world.created_unix)),
            );
            let height = canvas.text(
                &details,
                [area[0], y + canvas.r(CONTROL) + space(canvas, 1)],
                area[2] - area[0],
                CAPTION,
                TEXT_DIMMEST,
                false,
            )?;
            return Ok(y + canvas.r(CONTROL) + space(canvas, 1) + height);
        }
        Ok(y + canvas.r(CONTROL))
    })
}
