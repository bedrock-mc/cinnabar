use super::super::super::super::{IconRef, UiPresentationError, menu_artwork};
use super::super::{
    grid::space,
    icons::{self, Icon},
    motion::{Kind, Surface, mix, opacity},
    paint::{Bounds, Canvas},
    theme::{self, BODY, CAPTION, EDGE, HEADER5, TEXT, TEXT_DIMMER},
    widgets::{self, Variant},
};
use super::{PreviewArea, command, enabled};
use crate::menu::MenuView;
use launcher::dressing_room::{Action, DressingRoomSection};

pub(super) fn draw(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    b: Bounds,
    viewport: Bounds,
    preview: &mut PreviewArea,
) -> Result<f32, UiPresentationError> {
    let top = toolbar(canvas, view, b)?;
    collection(canvas, view, [b[0], top, b[2], b[3]], viewport, preview)
}

pub(super) fn toolbar(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    b: Bounds,
) -> Result<f32, UiPresentationError> {
    let [left, mut y, right, _] = b;
    let width = right - left;
    let cape_tab = view.dressing_room.section == DressingRoomSection::Capes;
    canvas.text_line(
        "WARDROBE",
        [left, y + canvas.r(0.2)],
        width - canvas.r(16.0),
        HEADER5,
        TEXT,
    )?;
    let description_height = canvas.text(
        "Choose your skin and cape.",
        [left, y + canvas.r(3.0)],
        width - canvas.r(16.0),
        CAPTION,
        TEXT_DIMMER,
        false,
    )?;
    let import_bounds = [right - canvas.r(14.8), y, right, y + canvas.r(4.4)];
    let import_action = command(if cape_tab {
        Action::ImportCape
    } else {
        Action::Import
    });
    let interaction = canvas.interaction(view, Some(import_action));
    widgets::button_face(
        canvas,
        import_bounds,
        Variant::Primary,
        if cape_tab {
            "+ Import cape"
        } else {
            "+ Import skin"
        },
        interaction,
        true,
    )?;
    if enabled(view) {
        canvas.hit(import_action, import_bounds)?;
    }
    y += canvas.r(6.0).max(canvas.r(4.4) + description_height);
    widgets::tabs(
        canvas,
        view,
        [left, y, right, y + canvas.r(4.4)],
        &[
            (
                "Skins",
                enabled(view).then_some(command(Action::SetSection(DressingRoomSection::Skins))),
            ),
            (
                "Capes",
                enabled(view).then_some(command(Action::SetSection(DressingRoomSection::Capes))),
            ),
        ],
        usize::from(cape_tab),
    )?;
    y += canvas.r(6.2);
    Ok(y)
}

pub(super) fn collection(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    b: Bounds,
    viewport: Bounds,
    preview: &mut PreviewArea,
) -> Result<f32, UiPresentationError> {
    let entrance = canvas.begin_entrance(Surface::Wardrobe(
        view.dressing_room.section == DressingRoomSection::Capes,
    ));
    let [left, mut y, right, _] = b;
    let width = right - left;
    let cape_tab = view.dressing_room.section == DressingRoomSection::Capes;
    if let Some(message) = &view.dressing_room.message {
        let pad = canvas.r(1.2);
        let height = canvas.measure_height(message, (width - pad * 2.0).max(1.0), CAPTION)?;
        let note = [left, y, right, y + height + pad * 2.0];
        canvas.fill(note, theme::NEUTRAL80.fill)?;
        canvas.frame(note, EDGE, theme::BORDER)?;
        canvas.text(
            message,
            [left + pad, y + pad],
            width - pad * 2.0,
            CAPTION,
            TEXT_DIMMER,
            false,
        )?;
        y = note[3] + space(canvas, 4);
    }
    if cape_tab {
        let no_cape = [left, y, right, y + canvas.r(5.8)];
        let selected = view.dressing_room.selected_cape.is_none();
        let action = command(Action::SelectCape(None));
        canvas.fill(
            no_cape,
            if selected {
                theme::PRIMARY_ROLE.shadow
            } else {
                theme::NEUTRAL80.fill
            },
        )?;
        canvas.frame(
            no_cape,
            EDGE,
            if selected {
                theme::PRIMARY_ROLE.fill
            } else {
                theme::BORDER
            },
        )?;
        icons::draw(
            canvas,
            if selected { Icon::Check } else { Icon::Cross },
            [left + canvas.r(1.2), y + canvas.r(2.0)],
            TEXT,
        )?;
        canvas.text_line(
            "No cape",
            [left + canvas.r(4.0), y + canvas.r(0.9)],
            width - canvas.r(5.0),
            BODY,
            TEXT,
        )?;
        canvas.text_line(
            "Keep your look simple",
            [left + canvas.r(4.0), y + canvas.r(3.3)],
            width - canvas.r(5.0),
            CAPTION,
            TEXT_DIMMER,
        )?;
        if enabled(view) {
            canvas.hit(action, no_cape)?;
        }
        if canvas.interaction(view, Some(action)).focused {
            canvas.frame(no_cape, EDGE, theme::OUTLINE)?;
        }
        y += canvas.r(8.0);
    }
    for imported in [false, true] {
        let count = if cape_tab {
            view.dressing_room
                .capes
                .iter()
                .filter(|entry| entry.imported == imported)
                .count()
        } else {
            view.dressing_room
                .skins
                .iter()
                .filter(|entry| entry.imported == imported)
                .count()
        };
        let title = match (cape_tab, imported) {
            (false, false) => "THE CLASSICS",
            (false, true) => "YOUR SKINS",
            (true, false) => "MINECRAFT CAPES",
            (true, true) => "YOUR CAPES",
        };
        canvas.text_line(title, [left, y], width, HEADER5, TEXT)?;
        y += canvas.r(3.2);
        let subtitle = match (cape_tab, imported) {
            (false, false) => "Two familiar faces. A fresh start.",
            (false, true) => "Your collection, your character.",
            (true, false) => "Iconic designs from Java Edition.",
            (true, true) => "The finishing touch, made by you.",
        };
        canvas.text_line(subtitle, [left, y], width, CAPTION, TEXT_DIMMER)?;
        y += canvas.r(3.0);
        if count == 0 {
            empty(
                canvas,
                view,
                [left, y, right, y + canvas.r(12.0)],
                cape_tab,
                imported,
            )?;
            y += canvas.r(14.4);
            continue;
        }
        let columns = if width < canvas.r(32.0) {
            1
        } else if (!imported && !cape_tab) || width < canvas.r(55.0) {
            2
        } else {
            3
        };
        let gap = space(canvas, 3);
        let cell_width = (width - gap * (columns - 1) as f32) / columns as f32;
        let cell_height = canvas.r(if cape_tab { 21.6 } else { 24.8 });
        let mut slot = 0;
        if cape_tab {
            for (index, entry) in view
                .dressing_room
                .capes
                .iter()
                .enumerate()
                .filter(|(_, entry)| entry.imported == imported)
            {
                let cell = cell(left, y, cell_width, cell_height, gap, columns, slot);
                if cell[1] < viewport[3] && cell[3] > viewport[1] {
                    preview.visible_capes.push(index);
                }
                card(
                    canvas,
                    view,
                    cell,
                    &entry.name,
                    if imported { "Custom cape" } else { "Minecraft" },
                    &menu_artwork::cape_thumbnail_key(&entry.id),
                    view.dressing_room.selected_cape == Some(index),
                    Action::SelectCape(Some(index)),
                )?;
                slot += 1;
            }
        } else {
            for (index, entry) in view
                .dressing_room
                .skins
                .iter()
                .enumerate()
                .filter(|(_, entry)| entry.imported == imported)
            {
                let cell = cell(left, y, cell_width, cell_height, gap, columns, slot);
                if cell[1] < viewport[3] && cell[3] > viewport[1] {
                    preview.visible_skins.push(index);
                }
                card(
                    canvas,
                    view,
                    cell,
                    &entry.name,
                    if entry.model == launcher::dressing_room::SkinModel::Slim {
                        "Slim arms"
                    } else {
                        "Classic arms"
                    },
                    &menu_artwork::thumbnail_key(&entry.id),
                    view.dressing_room.selected == Some(index),
                    Action::Select(index),
                )?;
                slot += 1;
            }
        }
        y += slot.div_ceil(columns) as f32 * (cell_height + gap) + canvas.r(2.4);
    }
    canvas.end_entrance(entrance, [viewport[2], viewport[3]])?;
    Ok(y)
}

fn cell(
    left: f32,
    top: f32,
    width: f32,
    height: f32,
    gap: f32,
    columns: usize,
    slot: usize,
) -> Bounds {
    let x = left + (width + gap) * (slot % columns) as f32;
    let y = top + (height + gap) * (slot / columns) as f32;
    [x, y, x + width, y + height]
}

fn empty(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    b: Bounds,
    cape: bool,
    imported: bool,
) -> Result<(), UiPresentationError> {
    canvas.fill(b, theme::NEUTRAL80.fill)?;
    canvas.bevel(b, theme::BEVEL_DARK, theme::BEVEL_LIGHT)?;
    let pad = canvas.r(1.6);
    canvas.text_line(
        if imported {
            "MAKE IT YOURS"
        } else {
            "Loading your collection…"
        },
        [b[0] + pad, b[1] + pad],
        b[2] - b[0] - pad * 2.0,
        BODY,
        TEXT,
    )?;
    canvas.text_line(
        if cape {
            "Import a PNG cape to complete your look."
        } else {
            "Import your first PNG skin."
        },
        [b[0] + pad, b[1] + canvas.r(4.2)],
        b[2] - b[0] - pad * 2.0,
        CAPTION,
        TEXT_DIMMER,
    )?;
    if imported {
        widgets::button(
            canvas,
            view,
            [b[0] + pad, b[1] + canvas.r(7.2), b[2] - pad, b[3] - pad],
            Variant::Secondary,
            if cape { "Import cape" } else { "Import skin" },
            enabled(view).then_some(command(if cape {
                Action::ImportCape
            } else {
                Action::Import
            })),
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn card(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    b: Bounds,
    name: &str,
    subtitle: &str,
    key: &str,
    selected: bool,
    action: Action,
) -> Result<(), UiPresentationError> {
    let action = command(action);
    let state = canvas.interaction(view, Some(action));
    let motion = canvas.feedback(state, enabled(view), selected, Kind::Surface);
    let role = canvas.role(theme::NEUTRAL);
    let fill = motion.color(role.fill, role.hovered, canvas.role(theme::NEUTRAL80).fill);
    canvas.fill(b, fill)?;
    canvas.frame(
        b,
        EDGE,
        mix(
            canvas.appearance.surface(theme::BORDER),
            theme::PRIMARY_ROLE.fill,
            motion.selected,
        ),
    )?;
    canvas.specular(b, theme::BEVEL_LIGHT, theme::BEVEL_DARK)?;
    let pad = canvas.r(1.2);
    let image_bottom = b[3] - canvas.r(6.8);
    let image = [b[0] + pad, b[1] + pad, b[2] - pad, image_bottom];
    canvas.fill(image, theme::NEUTRAL80.fill)?;
    if let Some(icon) = canvas.artwork.and_then(|art| art.get(key)).copied() {
        thumbnail(
            canvas,
            icon,
            [
                image[0] + pad,
                image[1] + canvas.r(0.6),
                image[2] - pad,
                image[3] - canvas.r(0.6),
            ],
        )?;
    }
    canvas.text_line(
        name,
        [b[0] + pad, image_bottom + canvas.r(1.2)],
        b[2] - b[0] - pad * 2.0,
        BODY,
        TEXT,
    )?;
    canvas.text_line(
        if selected { "Equipped" } else { subtitle },
        [b[0] + pad, image_bottom + canvas.r(3.8)],
        b[2] - b[0] - pad * 2.0,
        CAPTION,
        TEXT_DIMMER,
    )?;
    if selected {
        let badge = [
            image[2] - canvas.r(2.8),
            image[1],
            image[2],
            image[1] + canvas.r(2.8),
        ];
        canvas.fill(badge, theme::PRIMARY_ROLE.fill)?;
        icons::draw(
            canvas,
            Icon::Check,
            [badge[0] + canvas.r(0.6), badge[1] + canvas.r(0.6)],
            TEXT,
        )?;
    }
    if motion.focus > 0.0 {
        canvas.frame(b, EDGE, opacity(theme::OUTLINE, motion.focus))?;
    }
    if enabled(view) {
        canvas.hit(action, b)?;
    }
    Ok(())
}

fn thumbnail(canvas: &mut Canvas<'_>, icon: IconRef, b: Bounds) -> Result<(), UiPresentationError> {
    let width = (icon.uv[2] - icon.uv[0]) as f32;
    let height = (icon.uv[3] - icon.uv[1]) as f32;
    let scale = ((b[2] - b[0]) / width).min((b[3] - b[1]) / height);
    let left = (b[0] + b[2] - width * scale) * 0.5;
    let top = (b[1] + b[3] - height * scale) * 0.5;
    canvas.icon_ref(
        icon,
        [left, top, left + width * scale, top + height * scale],
    )
}
