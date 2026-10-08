//! Backend and terrain remain independent choices on the advanced world form.

use super::*;
use crate::local_worlds::backend_label;
use protocol::world_control::{Backend, UnavailableReason};

/// Draws the vanilla terrain controls independently of the server choice.
pub(super) fn terrain(
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
    sections::row(canvas, area, y, |canvas, area| {
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
            Generator::Flat => "A completely flat world stretching into the distance",
            Generator::Normal => "Standard Minecraft world creation based on a world seed",
        };
        caption(canvas, description, area, y + height + space(canvas, 2))
    })
}

/// Draws the explicitly chosen server, leaving unavailable BDS outside navigation.
pub(super) fn server(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    local_view: &WorldsView,
    area: Bounds,
) -> Result<f32, UiPresentationError> {
    let form = &local_view.create;
    let y = sections::row(canvas, area, area[1], |canvas, area| {
        let y = label(canvas, "Server", area, area[1])?;
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
        let middle = (area[0] + area[2]) * 0.5;
        let dragonfly = [area[0], y, middle, y + height];
        let bds = [middle, y, area[2], y + height];
        super::super::widgets::choice(
            canvas,
            view,
            dragonfly,
            backend_label(Backend::Dragonfly),
            form.backend == Backend::Dragonfly,
            local(A::Backend(Backend::Dragonfly)),
        )?;
        if local_view.bds_can_run {
            super::super::widgets::choice(
                canvas,
                view,
                bds,
                backend_label(Backend::Bds),
                form.backend == Backend::Bds,
                local(A::Backend(Backend::Bds)),
            )?;
            super::super::widgets::choice_focus(
                canvas,
                view,
                bds,
                form.backend == Backend::Bds,
                local(A::Backend(Backend::Bds)),
            )?;
        } else {
            button(
                canvas,
                view,
                bds,
                Variant::Secondary,
                backend_label(Backend::Bds),
                None,
            )?;
        }
        super::super::widgets::choice_focus(
            canvas,
            view,
            dragonfly,
            form.backend == Backend::Dragonfly,
            local(A::Backend(Backend::Dragonfly)),
        )?;
        let description = match (form.backend, local_view.bds_unavailable) {
            (_, Some(UnavailableReason::DockerMissing)) => {
                "BDS is unavailable: Docker is not installed. Dragonfly runs without Docker."
            }
            (_, Some(UnavailableReason::DockerNotRunning)) => {
                "Docker is not running. Start Docker before creating a BDS world, or select Dragonfly."
            }
            (_, Some(UnavailableReason::Other)) => {
                "BDS is unavailable on this platform. Select Dragonfly to continue."
            }
            (_, None) if !local_view.bds_can_run => {
                "Checking whether BDS can run on this computer."
            }
            (Backend::Dragonfly, _) => "Built-in local server. No Docker required.",
            (Backend::Bds, _) => "Official Bedrock Dedicated Server.",
        };
        let end = caption(canvas, description, area, y + height + space(canvas, 2))?;
        if !local_view.bds_can_run {
            let top = end + space(canvas, 2);
            button(
                canvas,
                view,
                [
                    area[0],
                    top,
                    (area[0] + canvas.r(20.0)).min(area[2]),
                    top + canvas.r(CONTROL),
                ],
                Variant::Secondary,
                "Check again",
                (!local_view.busy).then_some(local(A::RedetectBds)),
            )?;
            return Ok(top + canvas.r(CONTROL));
        }
        Ok(end)
    })?;
    Ok(y)
}

/// Keeps terrain controls available on the vanilla Advanced tab too.
pub(super) fn draw(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    local_view: &WorldsView,
    area: Bounds,
) -> Result<f32, UiPresentationError> {
    terrain(canvas, view, local_view, area)
}
