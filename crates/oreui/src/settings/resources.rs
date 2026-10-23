//! Global pack management uses OreUI cards over the staged library selection.

use super::super::paint::{Bounds, Canvas};
use super::super::theme::{self, BODY, CAPTION, EDGE, TEXT, TEXT_DIMMER};
use super::super::transitions::resources::{Card, CardItem, Cards, Group, Resources};
use super::super::widgets::{self, Variant};
use super::{Content, picker};
use crate::images::{BASE_PACK_IMAGE, CHEVRON_DOWN_IMAGE, CHEVRON_UP_IMAGE, MISSING_PACK_IMAGE};
use launcher::global_resources::{Action, Snapshot};
use launcher::menu::{MenuAction, MenuView};
use ui_engine::engine::screen::PaintError;

#[cfg(test)]
mod tests;

fn command(action: Action) -> MenuAction {
    MenuAction::GlobalResources(action)
}

pub(super) fn draw(content: &mut Content<'_, '_>) -> Result<(), PaintError> {
    let mut motion = content
        .canvas
        .transitions
        .as_deref_mut()
        .map(|transitions| std::mem::take(&mut transitions.resources))
        .unwrap_or_default();
    let animated = content
        .canvas
        .transitions
        .as_deref()
        .is_some_and(|t| t.motion.enabled());
    let snapshot = content.view.global_resources.clone();
    motion.sync(&snapshot, content.canvas.seconds, animated);
    let result = draw_sections(content, &snapshot, &motion);
    if let Some(transitions) = content.canvas.transitions.as_deref_mut() {
        transitions.resources = motion;
    }
    result
}

fn draw_sections(
    content: &mut Content<'_, '_>,
    snapshot: &Snapshot,
    motion: &Resources,
) -> Result<(), PaintError> {
    content.heading("menu.globalpacks", "Change how your worlds look.")?;
    toolbar(content, snapshot)?;
    reveal(content, motion.pending_fraction, false, |content| {
        note(
            content,
            "You have unapplied changes.",
            theme::PRIMARY_ROLE.fill,
        )
    })?;
    if !snapshot.message.is_empty() {
        note(content, &snapshot.message, theme::NEUTRAL80.fill)?;
    }
    accordion(
        content,
        &format!("Active packs ({})", snapshot.active.len() + 1),
        motion.expanded[0],
        command(Action::ToggleActive),
    )?;
    reveal(
        content,
        motion.expanded[0],
        snapshot.active_expanded,
        |content| {
            pack_cards(content, snapshot, motion, Group::Active)?;
            base_card(content)?;
            caption(content, "Packs at the top override packs below them.")
        },
    )?;
    content.y += content.canvas.r(1.2);
    accordion(
        content,
        &format!("My packs ({})", snapshot.available.len()),
        motion.expanded[1],
        command(Action::ToggleAvailable),
    )?;
    reveal(
        content,
        motion.expanded[1],
        snapshot.available_expanded,
        |content| {
            reveal(content, motion.empty_fraction, false, |content| {
                empty(
                    content,
                    "No imported packs",
                    "Import a .mcpack or .mcaddon to add your own textures.",
                )
            })?;
            pack_cards(content, snapshot, motion, Group::Available)
        },
    )?;
    content.y += content.canvas.r(2.4);
    Ok(())
}

/// Draws `draw` clipped to `fraction` of its height; hits survive only when `interactive`.
pub(super) fn reveal(
    content: &mut Content<'_, '_>,
    fraction: f32,
    interactive: bool,
    draw: impl FnOnce(&mut Content<'_, '_>) -> Result<(), PaintError>,
) -> Result<(), PaintError> {
    if fraction <= 0.0 {
        return Ok(());
    }
    if fraction >= 1.0 {
        let (hits, focus_hits, targets) = (
            content.canvas.hits.len(),
            content.canvas.focus_hits.len(),
            content.canvas.focus_targets.len(),
        );
        draw(content)?;
        if !interactive {
            content.canvas.hits.truncate(hits);
            content.canvas.focus_hits.truncate(focus_hits);
            content.canvas.focus_targets.truncate(targets);
        }
        return Ok(());
    }
    let scope = content
        .canvas
        .begin_reveal(content.inset(), content.y, fraction)?;
    draw(content)?;
    content.y = content.canvas.end_reveal(scope, content.y, interactive)?;
    Ok(())
}

/// Draws one group's retained cards, each fading as it moves between groups.
pub(super) fn cards<T: CardItem>(
    content: &mut Content<'_, '_>,
    motion: &Cards<T>,
    group: Group,
    mut draw: impl FnMut(&mut Content<'_, '_>, &Card<T>) -> Result<(), PaintError>,
) -> Result<(), PaintError> {
    for card in motion.cards.iter().filter(|card| card.group == group) {
        let previous = content.canvas.surface;
        content.canvas.surface = card.item.surface(group == Group::Active);
        reveal(content, card.visibility, card.present, |content| {
            draw(content, card)
        })?;
        content.canvas.surface = previous;
    }
    Ok(())
}

fn pack_cards(
    content: &mut Content<'_, '_>,
    snapshot: &Snapshot,
    motion: &Resources,
    group: Group,
) -> Result<(), PaintError> {
    cards(content, motion, group, |content, card| {
        pack_card(
            content,
            snapshot,
            &card.item,
            group == Group::Active,
            card.index,
            card.details,
            card.present,
        )
    })
}

fn toolbar(content: &mut Content<'_, '_>, snapshot: &Snapshot) -> Result<(), PaintError> {
    actions(
        content,
        &[
            (
                "Import pack",
                Variant::Secondary,
                (!snapshot.busy).then_some(command(Action::Import)),
            ),
            (
                "Apply changes",
                Variant::Primary,
                snapshot.can_apply().then_some(command(Action::Apply)),
            ),
        ],
    )?;
    content.y += content.canvas.r(1.2);
    Ok(())
}

pub(super) fn caption(content: &mut Content<'_, '_>, text: &str) -> Result<(), PaintError> {
    let [left, right] = content.inset();
    let height = content.canvas.text(
        text,
        [left, content.y],
        right - left,
        CAPTION,
        TEXT_DIMMER,
        false,
    )?;
    content.y += height.max(content.canvas.r(CAPTION.line)) + content.canvas.r(0.8);
    Ok(())
}

pub(super) fn note(
    content: &mut Content<'_, '_>,
    text: &str,
    fill: theme::Rgba,
) -> Result<(), PaintError> {
    let [left, right] = content.inset();
    let pad = content.canvas.r(1.2);
    let height =
        content
            .canvas
            .measure_height(text, (right - left - 2.0 * pad).max(1.0), CAPTION)?;
    let bounds = [left, content.y, right, content.y + height + 2.0 * pad];
    content.canvas.fill(bounds, fill)?;
    content.canvas.frame(bounds, EDGE, theme::BORDER)?;
    content.canvas.text(
        text,
        [left + pad, content.y + pad],
        right - left - 2.0 * pad,
        CAPTION,
        TEXT,
        false,
    )?;
    content.y = bounds[3] + content.canvas.r(1.2);
    Ok(())
}

/// A group header whose chevron follows the group's disclosure `fraction`.
pub(super) fn accordion(
    content: &mut Content<'_, '_>,
    label: &str,
    fraction: f32,
    action: MenuAction,
) -> Result<(), PaintError> {
    let [left, right] = content.inset();
    let bounds = [left, content.y, right, content.y + content.canvas.r(4.8)];
    let state = content.canvas.interaction(content.view, Some(action));
    let motion = content
        .canvas
        .feedback(state, true, false, super::super::motion::Kind::Surface);
    let role = content.canvas.role(theme::NEUTRAL80);
    content
        .canvas
        .fill(bounds, motion.color(role.fill, role.hovered, role.pressed))?;
    content.canvas.frame(bounds, EDGE, theme::BORDER)?;
    content
        .canvas
        .specular(bounds, role.specular[0], role.specular[1])?;
    let pad = content.canvas.r(1.2);
    content.canvas.text_line(
        label,
        [left + pad, content.y + content.canvas.r(1.4)],
        right - left - 2.0 * pad - content.canvas.r(2.4),
        BODY,
        TEXT,
    )?;
    chevron(
        content.canvas,
        [
            right - pad - content.canvas.r(1.2),
            (bounds[1] + bounds[3]) * 0.5,
        ],
        fraction,
    )?;
    if motion.focus > 0.0 {
        content.canvas.frame(
            bounds,
            EDGE,
            super::super::motion::opacity(theme::OUTLINE, motion.focus),
        )?;
    }
    content.canvas.hit(action, bounds)?;
    content.y = bounds[3] + content.canvas.r(0.8);
    Ok(())
}

fn chevron(canvas: &mut Canvas<'_>, centre: [f32; 2], fraction: f32) -> Result<(), PaintError> {
    let pixel = canvas.r(EDGE);
    let bounds = [
        centre[0] - 3.5 * pixel,
        centre[1] - 2.0 * pixel,
        centre[0] + 3.5 * pixel,
        centre[1] + 2.0 * pixel,
    ];
    if canvas.rotated_masked_sprite(
        if fraction >= 1.0 {
            CHEVRON_UP_IMAGE
        } else {
            CHEVRON_DOWN_IMAGE
        },
        bounds,
        TEXT,
        if fraction >= 1.0 {
            0.0
        } else {
            std::f32::consts::PI * fraction
        },
    )? {
        return Ok(());
    }
    let up = fraction > 0.5;
    for row in 0..4 {
        let y = bounds[1] + if up { 3 - row } else { row } as f32 * pixel;
        let inset = row as f32 * pixel;
        canvas.fill(
            [bounds[0] + inset, y, bounds[0] + inset + pixel, y + pixel],
            TEXT,
        )?;
        canvas.fill(
            [bounds[2] - inset - pixel, y, bounds[2] - inset, y + pixel],
            TEXT,
        )?;
    }
    Ok(())
}

/// A card's artwork icon, else the base-pack or missing-icon placeholder.
fn thumbnail(
    content: &mut Content<'_, '_>,
    bounds: Bounds,
    icon: Option<&str>,
    base: bool,
) -> Result<(), PaintError> {
    content.canvas.fill(bounds, theme::NEUTRAL80.fill)?;
    let icon = icon.and_then(|path| content.canvas.artwork?.get(path).copied());
    if let Some(icon) = icon {
        let source = [
            (icon.uv[2] - icon.uv[0]) as f32,
            (icon.uv[3] - icon.uv[1]) as f32,
        ];
        let scale = ((bounds[2] - bounds[0]) / source[0].max(1.0))
            .min((bounds[3] - bounds[1]) / source[1].max(1.0));
        let size = source.map(|side| side * scale);
        let at = [
            (bounds[0] + bounds[2] - size[0]) * 0.5,
            (bounds[1] + bounds[3] - size[1]) * 0.5,
        ];
        content
            .canvas
            .icon_ref(icon, [at[0], at[1], at[0] + size[0], at[1] + size[1]])?;
    } else if !content.canvas.sprite(
        if base {
            BASE_PACK_IMAGE
        } else {
            MISSING_PACK_IMAGE
        },
        bounds,
        [255; 4],
    )? {
        content.canvas.text_centred(
            if base { "V" } else { "?" },
            bounds,
            theme::SECTION_HEADER,
            TEXT_DIMMER,
            false,
        )?;
    }
    content.canvas.frame(bounds, EDGE, theme::BORDER)
}

fn base_card(content: &mut Content<'_, '_>) -> Result<(), PaintError> {
    let [left, right] = content.inset();
    let height = content.canvas.r(7.2);
    let bounds = [left, content.y, right, content.y + height];
    widgets::row(content.canvas, content.view, bounds, false, None)?;
    let pad = content.canvas.r(1.2);
    let side = content.canvas.r(4.8);
    thumbnail(
        content,
        [
            left + pad,
            content.y + pad,
            left + pad + side,
            content.y + pad + side,
        ],
        None,
        true,
    )?;
    let text_left = left + side + 2.0 * pad;
    let width = (right - text_left - pad).max(1.0);
    content.canvas.text_line(
        "Vanilla Textures",
        [text_left, content.y + content.canvas.r(1.6)],
        width,
        BODY,
        TEXT,
    )?;
    content.canvas.text_line(
        "Base pack · Always active",
        [text_left, content.y + content.canvas.r(3.6)],
        width,
        CAPTION,
        TEXT_DIMMER,
    )?;
    content.y = bounds[3] + content.canvas.r(0.8);
    Ok(())
}

/// A library card's header row; details reveal `body` under it inside the card's frame.
pub(super) struct Face<'a> {
    pub(super) icon: Option<&'a str>,
    pub(super) title: &'a str,
    pub(super) subtitle: &'a str,
    pub(super) expanded: bool,
    pub(super) toggle: MenuAction,
    pub(super) details: f32,
    pub(super) interactive: bool,
}

/// Draws a card's header and its revealed details, whose contents `body` draws.
pub(super) fn card(
    content: &mut Content<'_, '_>,
    face: Face<'_>,
    body: impl FnOnce(&mut Content<'_, '_>) -> Result<(), PaintError>,
) -> Result<(), PaintError> {
    card_with_summary(content, face, |_| Ok(()), body)
}

/// Draws a card with permanent controls before its optional revealed details.
pub(super) fn card_with_summary(
    content: &mut Content<'_, '_>,
    face: Face<'_>,
    summary: impl FnOnce(&mut Content<'_, '_>) -> Result<(), PaintError>,
    body: impl FnOnce(&mut Content<'_, '_>) -> Result<(), PaintError>,
) -> Result<(), PaintError> {
    let [left, right] = content.inset();
    let header = [left, content.y, right, content.y + content.canvas.r(7.2)];
    widgets::row(
        content.canvas,
        content.view,
        header,
        face.expanded,
        face.interactive.then_some(face.toggle),
    )?;
    let pad = content.canvas.r(1.2);
    let side = content.canvas.r(4.8);
    thumbnail(
        content,
        [
            left + pad,
            content.y + pad,
            left + pad + side,
            content.y + pad + side,
        ],
        face.icon,
        false,
    )?;
    let text_left = left + side + 2.0 * pad;
    let width = (right - text_left - pad - content.canvas.r(2.4)).max(1.0);
    content.canvas.text_line(
        face.title,
        [text_left, content.y + content.canvas.r(1.6)],
        width,
        BODY,
        TEXT,
    )?;
    content.canvas.text_line(
        face.subtitle,
        [text_left, content.y + content.canvas.r(3.6)],
        width,
        CAPTION,
        TEXT_DIMMER,
    )?;
    chevron(
        content.canvas,
        [
            right - pad - content.canvas.r(1.2),
            (header[1] + header[3]) * 0.5,
        ],
        face.details,
    )?;
    content.y = header[3];
    let top = content.y;
    let background = content.canvas.nodes.len();
    content.canvas.fill(
        [left, top, right, top + content.canvas.r(EDGE)],
        theme::NEUTRAL80.fill,
    )?;
    let span = content.span;
    content.span = [span[0] + pad, span[1] - pad];
    content.y += pad;
    summary(content)?;
    if content.y == top + pad {
        content.y = top;
    }
    reveal(
        content,
        face.details,
        face.expanded && face.interactive,
        |content| {
            if content.y == top {
                content.y += pad;
            }
            body(content)
        },
    )?;
    if content.y > top {
        content.y += pad;
        content
            .canvas
            .resize_fill(background, [left, top, right, content.y])?;
        content
            .canvas
            .frame([left, top, right, content.y], EDGE, theme::BORDER)?;
    } else {
        content.canvas.nodes.truncate(background);
    }
    content.span = span;
    content.y += content.canvas.r(0.8);
    Ok(())
}

fn pack_card(
    content: &mut Content<'_, '_>,
    snapshot: &Snapshot,
    pack: &resource_pack::InstalledPack,
    active: bool,
    index: usize,
    details: f32,
    interactive: bool,
) -> Result<(), PaintError> {
    let version = pack.version.map(|part| part.to_string()).join(".");
    let subtitle = if active {
        format!("Priority {} · v{version}", index + 1)
    } else {
        format!("Ready to activate · v{version}")
    };
    let face = Face {
        icon: snapshot
            .icons
            .get(&(pack.id.to_string(), pack.revision))
            .map(String::as_str),
        title: &pack.name,
        subtitle: &subtitle,
        expanded: snapshot.details_expanded == Some((active, index)),
        toggle: command(Action::ReadMore(active, index)),
        details,
        interactive,
    };
    card(content, face, |content| {
        let [inner_left, inner_right] = content.inset();
        if !pack.description.is_empty() {
            caption(content, &pack.description)?;
        }
        if active && !pack.subpacks.is_empty() {
            let variant = snapshot
                .selection
                .get(index)
                .and_then(|selection| {
                    pack.subpacks
                        .iter()
                        .find(|variant| variant.folder == selection.subpack)
                })
                .map(|variant| variant.name.as_str())
                .unwrap_or("Default textures");
            let bounds = [
                inner_left,
                content.y + content.canvas.r(0.4),
                inner_right,
                content.y + content.canvas.r(5.0),
            ];
            if interactive
                && !snapshot.busy
                && pack
                    .subpacks
                    .iter()
                    .any(|variant| variant.memory_tier <= snapshot.memory_tier)
            {
                picker::select(
                    content.canvas,
                    content.view,
                    bounds,
                    &format!("Variant: {variant}"),
                    command(Action::Settings(index)),
                )?;
            } else {
                widgets::button(
                    content.canvas,
                    content.view,
                    bounds,
                    Variant::Secondary,
                    &format!("Variant: {variant}"),
                    None,
                )?;
            }
            content.y = bounds[3] + content.canvas.r(1.2);
        }
        let available = interactive && !snapshot.busy;
        let mut buttons = Vec::new();
        if active {
            if index > 0 {
                buttons.push((
                    "Move up",
                    Variant::Neutral,
                    available.then_some(command(Action::MoveUp(index))),
                ));
            }
            if index + 1 < snapshot.active.len() {
                buttons.push((
                    "Move down",
                    Variant::Neutral,
                    available.then_some(command(Action::MoveDown(index))),
                ));
            }
            buttons.push((
                "Deactivate",
                Variant::Secondary,
                available.then_some(command(Action::Deactivate(index))),
            ));
        } else {
            buttons.push((
                "Activate",
                Variant::Primary,
                available.then_some(command(Action::Activate(index))),
            ));
        }
        actions(content, &buttons)
    })
}

/// The empty-library panel: a title over a one-line explanation.
pub(super) fn empty(
    content: &mut Content<'_, '_>,
    title: &str,
    text: &str,
) -> Result<(), PaintError> {
    let [left, right] = content.inset();
    let pad = content.canvas.r(1.2);
    let width = (right - left - 2.0 * pad).max(1.0);
    let description = content.canvas.measure_height(text, width, CAPTION)?;
    let bounds = [
        left,
        content.y,
        right,
        content.y + description + content.canvas.r(4.8),
    ];
    widgets::panel(content.canvas, bounds)?;
    content
        .canvas
        .text_line(title, [left + pad, content.y + pad], width, BODY, TEXT)?;
    content.canvas.text(
        text,
        [left + pad, content.y + content.canvas.r(3.2)],
        width,
        CAPTION,
        TEXT_DIMMER,
        false,
    )?;
    content.y = bounds[3] + content.canvas.r(0.8);
    Ok(())
}

/// Lays buttons out in as many equal columns as their labels fit.
pub(super) fn actions(
    content: &mut Content<'_, '_>,
    buttons: &[(&str, Variant, Option<MenuAction>)],
) -> Result<(), PaintError> {
    let [left, right] = content.inset();
    let gap = content.canvas.r(0.8);
    let width = (right - left).max(1.0);
    let minimum = buttons.iter().try_fold(0.0_f32, |minimum, (label, _, _)| {
        content
            .canvas
            .measure(label, BODY)
            .map(|label| minimum.max(label + content.canvas.r(3.2)))
    })?;
    let columns = ((width + gap) / (minimum + gap)).floor().max(1.0) as usize;
    let columns = columns.min(buttons.len().max(1));
    let cell = (width - gap * (columns - 1) as f32) / columns as f32;
    let height = content.canvas.r(4.4);
    for (index, (label, variant, action)) in buttons.iter().enumerate() {
        let x = left + (index % columns) as f32 * (cell + gap);
        let y = content.y + (index / columns) as f32 * (height + gap);
        widgets::button(
            content.canvas,
            content.view,
            [x, y, x + cell, y + height],
            *variant,
            label,
            *action,
        )?;
    }
    let rows = buttons.len().div_ceil(columns);
    content.y += height * rows as f32 + gap * rows.saturating_sub(1) as f32;
    Ok(())
}

pub(super) fn draw_picker(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    size: [f32; 2],
) -> Result<bool, PaintError> {
    let Some(picker) = variant_picker(&view.global_resources) else {
        return Ok(false);
    };
    picker::draw_choices(canvas, view, size, picker)?;
    Ok(true)
}

fn variant_picker(snapshot: &Snapshot) -> Option<picker::Picker> {
    let (index, pack) = snapshot
        .settings
        .and_then(|index| snapshot.active.get(index).map(|pack| (index, pack)))?;
    let selected = snapshot
        .selection
        .get(index)
        .map(|selection| selection.subpack.as_str());
    let selected = pack
        .subpacks
        .iter()
        .position(|variant| Some(variant.folder.as_str()) == selected)
        .unwrap_or(usize::MAX);
    let choices: Vec<_> = pack
        .subpacks
        .iter()
        .enumerate()
        .filter(|(_, variant)| variant.memory_tier <= snapshot.memory_tier)
        .collect();
    if choices.is_empty() {
        return None;
    }
    let current = choices
        .iter()
        .position(|(index, _)| *index == selected)
        .unwrap_or(usize::MAX);
    Some(picker::Picker {
        title: format!("{} · Variant", pack.name),
        labels: choices
            .iter()
            .map(|(_, variant)| variant.name.clone())
            .collect(),
        actions: choices
            .iter()
            .map(|(index, _)| command(Action::Subpack(*index)))
            .collect(),
        selected: current,
        close: command(Action::CloseSettings),
        scroll_key: format!("oreui_pack_variant/{}", pack.id),
    })
}
