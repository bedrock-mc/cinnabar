//! Backend and terrain remain independent choices on the advanced world form.

use launcher::local_worlds::backend_label;
use protocol::world_control::{Backend, UnavailableReason};
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
) -> Result<f32, UiPresentationError> {
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
        let y = label(canvas, "Backend", area, area[1])?;
        let options: Vec<_> = [Backend::Dragonfly, Backend::Bds]
            .into_iter()
            .map(|backend| {
                (
                    backend_label(backend),
                    local(A::Backend(backend)),
                    form.backend == backend,
                )
            })
            .collect();
        let height = choice_height(canvas, area, &options)?;
        segmented(canvas, view, [area[0], y, area[2], y + height], &options)?;
        let description = match (form.backend, local_view.bds_unavailable) {
            (Backend::Dragonfly, _) => "Built-in local server. No Docker required.",
            (Backend::Bds, Some(UnavailableReason::DockerMissing)) => {
                "BDS needs Docker on Mac. Install Docker, or select Dragonfly to continue without it."
            }
            (Backend::Bds, Some(UnavailableReason::DockerNotRunning)) => {
                "Docker is not running. Start Docker before creating a BDS world, or select Dragonfly."
            }
            (Backend::Bds, _) if !local_view.bds_can_run => {
                "BDS is unavailable on this computer. Select Dragonfly to continue."
            }
            (Backend::Bds, _) => "Official Bedrock Dedicated Server.",
        };
        caption(canvas, description, area, y + height + space(canvas, 2))
    })?;
    sections::row(canvas, area, y, |canvas, area| {
        let y = label(canvas, "World generator", area, area[1])?;
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
    })
}
