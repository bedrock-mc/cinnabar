//! Terrain and advanced world options for a new BDS world.

use {
    super::*,
    launcher::local_worlds::{WorldsView, world_type_label},
    launcher::menu::{LocalWorldAction as A, MenuField, MenuView},
};

pub(super) fn draw(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    local_view: &WorldsView,
    area: Bounds,
) -> Result<f32, PaintError> {
    let form = &local_view.create;
    let y = sections::row(canvas, area, area[1], |canvas, area| {
        let y = label(canvas, "World seed", area, area[1])?;
        let button_width = canvas.r(12.0).min((area[2] - area[0]) * 0.3);
        let field = [
            area[0],
            y,
            area[2] - button_width - space(canvas, 1),
            y + canvas.r(FIELD),
        ];
        text_field_on_panel(
            canvas,
            view,
            field,
            &form.seed_text,
            "3257840388504953787",
            view.field == Some(MenuField::WorldSeed),
            Some(local(A::SeedField)),
        )?;
        button(
            canvas,
            view,
            [field[2] + space(canvas, 1), y, area[2], field[3]],
            Variant::Secondary,
            "Templates",
            None,
        )?;
        caption(
            canvas,
            "Guides the algorithm that magically creates your world",
            area,
            field[3] + space(canvas, 1),
        )
    })?;
    let y = sections::row(canvas, area, y, |canvas, area| {
        let y = label(canvas, "World type", area, area[1])?;
        let options: Vec<_> = [Generator::Normal, Generator::Flat]
            .into_iter()
            .map(|generator| {
                (
                    world_type_label(generator),
                    local(A::Flat(generator == Generator::Flat)),
                    form.generator == generator,
                )
            })
            .collect();
        let height = choice_height(canvas, area, &options)?;
        segmented(canvas, view, [area[0], y, area[2], y + height], &options)?;
        let description = match form.generator {
            Generator::Flat => "A flat world to build up or mine down into",
            Generator::Normal => {
                "Explore mountains, caves and biomes in a naturally generated world."
            }
        };
        caption(canvas, description, area, y + height + space(canvas, 2))
    })?;
    options::draw(canvas, view, local_view, area, y, false)
}

/// Shows the saved seed and terrain without allowing generation settings to change.
pub(super) fn edit(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    local_view: &WorldsView,
    area: Bounds,
) -> Result<f32, PaintError> {
    let Some(world) = local_view.edited.as_ref() else {
        return Ok(area[1]);
    };
    let y = sections::row(canvas, area, area[1], |canvas, inner| {
        let y = label(canvas, "World seed", inner, inner[1])?;
        caption(canvas, &world.seed.to_string(), inner, y)
    })?;
    let y = sections::row(canvas, area, y, |canvas, inner| {
        let y = label(canvas, "World type", inner, inner[1])?;
        caption(canvas, world_type_label(world.generator), inner, y)
    })?;
    options::draw(canvas, view, local_view, area, y, true)
}
