use super::super::super::super::UiPresentationError;
use super::super::{
    grid::space,
    paint::{Bounds, Canvas},
    theme::{self, BODY, CAPTION, EDGE, HEADER5, TEXT, TEXT_DIMMER},
    widgets::{self, Variant},
};
use super::{PreviewArea, command, enabled};
use launcher::dressing_room::{Action, DressingRoomSection, SkinModel};
use launcher::menu::MenuView;

pub(super) fn draw(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    b: Bounds,
    viewport: Bounds,
) -> Result<PreviewArea, UiPresentationError> {
    widgets::panel(canvas, b)?;
    let pad = space(canvas, 4);
    let [left, right] = [b[0] + pad, b[2] - pad];
    let width = right - left;
    canvas.text_line("YOUR LOOK", [left, b[1] + pad], width, HEADER5, TEXT)?;
    let badge_width = canvas.r(9.2).min(width * 0.38);
    let badge = [
        right - badge_width,
        b[1] + pad - canvas.r(0.2),
        right,
        b[1] + pad + canvas.r(2.2),
    ];
    canvas.fill(badge, theme::PRIMARY_ROLE.shadow)?;
    canvas.text_centred("EQUIPPED", badge, CAPTION, TEXT, false)?;
    let cape_tab = view.dressing_room.section == DressingRoomSection::Capes;
    let custom = if cape_tab {
        view.dressing_room
            .selected_cape()
            .is_some_and(|cape| cape.imported)
    } else {
        view.dressing_room
            .selected_skin()
            .is_some_and(|skin| skin.imported)
    };
    let details_height = if !cape_tab && custom {
        18.2
    } else if custom {
        12.2
    } else {
        8.4
    };
    let details_top = b[3] - pad - canvas.r(details_height);
    let stage = [
        left,
        b[1] + pad + canvas.r(3.6),
        right,
        details_top - canvas.r(4.2),
    ];
    canvas.fill(stage, theme::NEUTRAL80.fill)?;
    canvas.bevel(stage, theme::BEVEL_DARK, theme::BEVEL_LIGHT)?;
    let floor_y = stage[3] - canvas.r(1.0);
    let control = [
        left + canvas.r(1.6),
        stage[1] + canvas.r(1.4),
        right - canvas.r(1.6),
        floor_y - canvas.r(0.6),
    ];
    let hint_bottom = details_top - canvas.r(0.4);
    canvas.text_centred_visible(
        "Drag to rotate",
        [left, stage[3], right, hint_bottom],
        CAPTION,
        TEXT_DIMMER,
    )?;
    canvas.fill(
        [left, hint_bottom, right, hint_bottom + canvas.r(EDGE)],
        theme::BEVEL_LIGHT,
    )?;
    let (name, caption) = if cape_tab {
        view.dressing_room
            .selected_cape()
            .map_or(("No cape", "A clean silhouette."), |cape| {
                (
                    cape.name.as_str(),
                    if cape.imported {
                        "Custom cape"
                    } else {
                        "Minecraft cape"
                    },
                )
            })
    } else {
        view.dressing_room
            .selected_skin()
            .map_or(("Your skin", "Current look"), |skin| {
                (
                    skin.name.as_str(),
                    if skin.imported {
                        "Custom skin"
                    } else {
                        "Minecraft classic"
                    },
                )
            })
    };
    canvas.text_line(name, [left, details_top + canvas.r(0.8)], width, BODY, TEXT)?;
    canvas.text_line(
        caption,
        [left, details_top + canvas.r(3.2)],
        width,
        CAPTION,
        TEXT_DIMMER,
    )?;
    if !cape_tab {
        let skin = view.dressing_room.selected_skin();
        let mut y = details_top + canvas.r(6.2);
        if custom && skin.is_some_and(|skin| skin.model != SkinModel::Custom) {
            widgets::tabs(
                canvas,
                view,
                [left, y, right, y + canvas.r(4.4)],
                &[
                    (
                        "Classic arms",
                        enabled(view).then_some(command(Action::SetModel(SkinModel::Classic))),
                    ),
                    (
                        "Slim arms",
                        enabled(view).then_some(command(Action::SetModel(SkinModel::Slim))),
                    ),
                ],
                usize::from(skin.is_some_and(|skin| skin.model == SkinModel::Slim)),
            )?;
            y += canvas.r(6.0);
        }
        if custom && let Some(index) = view.dressing_room.selected {
            manage(
                canvas,
                view,
                [left, y, right, y + canvas.r(4.4)],
                Action::BeginRename(index),
                Action::BeginDelete(index),
            )?;
        } else {
            canvas.text_line(
                if skin.is_some_and(|skin| skin.model == SkinModel::Slim) {
                    "Slim arms"
                } else {
                    "Classic arms"
                },
                [left, y],
                width,
                CAPTION,
                TEXT_DIMMER,
            )?;
        }
    } else if custom && let Some(index) = view.dressing_room.selected_cape {
        manage(
            canvas,
            view,
            [
                left,
                details_top + canvas.r(6.2),
                right,
                details_top + canvas.r(10.6),
            ],
            Action::BeginRenameCape(index),
            Action::BeginDeleteCape(index),
        )?;
    }
    Ok(PreviewArea {
        control,
        clip: [
            stage[0].max(viewport[0]),
            stage[1].max(viewport[1]),
            stage[2].min(viewport[2]),
            stage[3].min(viewport[3]),
        ],
        visible_skins: Vec::new(),
        visible_capes: Vec::new(),
    })
}

fn manage(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    b: Bounds,
    rename: Action,
    remove: Action,
) -> Result<(), UiPresentationError> {
    let mid = (b[0] + b[2]) * 0.5;
    let gap = canvas.r(0.6);
    widgets::button(
        canvas,
        view,
        [b[0], b[1], mid - gap, b[3]],
        Variant::Secondary,
        "Rename",
        enabled(view).then_some(command(rename)),
    )?;
    widgets::button(
        canvas,
        view,
        [mid + gap, b[1], b[2], b[3]],
        Variant::Neutral,
        "Remove",
        enabled(view).then_some(command(remove)),
    )
}
