//! Video renderer choices and the shader-pack library use separate settings tabs.

use super::super::icons::Icon;
use super::super::paint::{Bounds, Canvas};
use super::super::theme::{self, BODY, CAPTION, EDGE, TEXT, TEXT_DIMMER};
use super::super::widgets::{self, Variant};
use super::Content;
use launcher::menu::{
    MenuAction, RendererChoice,
    settings_storage::{StorageAction, StorageFolder},
    shader_pack_status::pack_label,
};
use launcher::shader_options::Action;
use ui_engine::engine::screen::PaintError;

#[cfg(test)]
mod tests;

/// Keeps the renderer switch in Video and links to the selected shader pack.
pub(super) fn draw(content: &mut Content<'_, '_>) -> Result<(), PaintError> {
    if render_model::ENHANCED_RENDERING_ENABLED {
        let description = if content.view.render_mode == ui::RenderMode::ShaderPack {
            format!(
                "Shader pack active: {}. Select Vanilla or Enhanced to turn shaders off.",
                content
                    .view
                    .shader_pack
                    .as_deref()
                    .map_or("Unknown", pack_label)
            )
        } else {
            "Enhanced improves lighting, shadows, sky and reflections.".to_owned()
        };
        picker(
            content,
            "Renderer",
            &description,
            [
                (RendererChoice::Vanilla, "Vanilla"),
                (RendererChoice::Enhanced, "Enhanced"),
            ]
            .into_iter(),
            content.view.renderer,
        )?;
    }
    if render_model::SHADER_PACKS_ENABLED {
        let selected = (content.view.render_mode == ui::RenderMode::ShaderPack)
            .then_some(content.view.shader_pack.as_deref())
            .flatten()
            .map_or("Off", pack_label);
        super::services::action_row(
            content,
            &format!("Shaders: {selected}"),
            "",
            "Open",
            Some(MenuAction::SettingsShaders),
        )?;
    }
    Ok(())
}

/// Draws Off and every pack as one pick-one list, ending in the drop zone that adds packs.
pub(super) fn shaders(content: &mut Content<'_, '_>) -> Result<(), PaintError> {
    if !render_model::SHADER_PACKS_ENABLED {
        return Ok(());
    }
    let view = content.view;
    content.heading(
        "Shader pack",
        "Change your world's lighting with Java shader packs.",
    )?;
    banner(
        content,
        "Shader packs are experimental. If a pack can't load, the game uses Vanilla rendering.",
    )?;
    let active = view.render_mode == ui::RenderMode::ShaderPack;
    let removing = view.shader_pack_removing.as_deref();
    let status = if view.renderer_failure.is_some() {
        Chip::Failed
    } else if view.shader_pack_loading {
        Chip::Loading
    } else {
        Chip::Active
    };
    entry(
        content,
        Entry {
            title: "Off",
            caption: Some("Vanilla rendering"),
            icon: Icon::Off,
            chips: &[],
            selected: !active,
            select: Some(MenuAction::SelectRenderer(RendererChoice::Vanilla)),
            remove: None,
            controls: false,
        },
    )?;
    if active && view.renderer.is_none() {
        let name = view.shader_pack.as_deref().unwrap_or("Selected pack");
        entry(
            content,
            Entry {
                title: pack_label(name),
                caption: Some("Not in your shader pack library"),
                icon: Icon::Sun,
                chips: &[status],
                selected: true,
                select: None,
                remove: None,
                controls: true,
            },
        )?;
    }
    // Only a shared label needs the kind to tell the entries apart.
    let mut labels = std::collections::HashMap::<&str, usize>::new();
    for name in view.shader_packs.iter() {
        *labels.entry(pack_label(name)).or_default() += 1;
    }
    for (index, name) in view.shader_packs.iter().enumerate() {
        let Ok(index) = u16::try_from(index) else {
            continue;
        };
        let choice = RendererChoice::ShaderPack(index, view.shader_packs_revision);
        let selected = active && view.renderer == Some(choice);
        let label = pack_label(name);
        let mut chips = Vec::new();
        if removing == Some(name.as_str()) {
            chips.push(Chip::Removing);
        } else if selected {
            chips.push(status);
        }
        if labels.get(label).is_some_and(|count| *count > 1) {
            chips.push(if label.len() < name.len() {
                Chip::Zip
            } else {
                Chip::Folder
            });
        }
        entry(
            content,
            Entry {
                title: label,
                caption: None,
                icon: Icon::Sun,
                chips: &chips,
                selected,
                select: (removing != Some(name.as_str()))
                    .then_some(MenuAction::SelectRenderer(choice)),
                remove: selected.then_some(MenuAction::RemoveShaderPackDialog(
                    index,
                    view.shader_packs_revision,
                )),
                controls: selected,
            },
        )?;
    }
    drop_zone(content)
}

/// A short state or kind label beside a pack's name.
#[derive(Clone, Copy)]
enum Chip {
    Active,
    Loading,
    Failed,
    Removing,
    Zip,
    Folder,
}

impl Chip {
    fn label(self) -> &'static str {
        match self {
            Self::Active => "Active",
            Self::Loading => "Loading…",
            Self::Failed => "Failed",
            Self::Removing => "Removing…",
            Self::Zip => "ZIP",
            Self::Folder => "Folder",
        }
    }

    fn width(self, canvas: &mut Canvas<'_>) -> Result<f32, PaintError> {
        Ok(canvas.measure(self.label(), CAPTION)? + canvas.r(1.6))
    }

    /// The chip's size within `limit`; a narrower chip wraps its label onto more lines.
    fn size(self, canvas: &mut Canvas<'_>, limit: f32) -> Result<[f32; 2], PaintError> {
        let width = self.width(canvas)?.min(limit.max(canvas.r(2.0)));
        let label = canvas.measure_height(self.label(), width - canvas.r(1.6), CAPTION)?;
        Ok([width, label.max(canvas.r(2.0))])
    }

    /// Draws the chip filling `bounds` from [`Self::size`].
    fn draw(self, canvas: &mut Canvas<'_>, bounds: Bounds) -> Result<(), PaintError> {
        let [fill, text] = match self {
            Self::Active => [theme::PRIMARY_ROLE.fill, TEXT],
            Self::Failed => [theme::DESTRUCTIVE.fill, TEXT],
            Self::Loading | Self::Removing | Self::Zip | Self::Folder => {
                [theme::NEUTRAL80.fill, TEXT_DIMMER]
            }
        };
        canvas.fill(bounds, fill)?;
        canvas.frame(bounds, EDGE, theme::BORDER)?;
        let width = bounds[2] - bounds[0] - canvas.r(1.6);
        let height = canvas.measure_height(self.label(), width, CAPTION)?;
        canvas.centered_wrapped_text(
            self.label(),
            [
                bounds[0] + canvas.r(0.8),
                (bounds[1] + bounds[3] - height) * 0.5,
            ],
            width,
            CAPTION,
            text,
        )?;
        Ok(())
    }
}

/// One library row; the selected row becomes a card holding its pack's controls.
struct Entry<'a> {
    title: &'a str,
    caption: Option<&'a str>,
    icon: Icon,
    chips: &'a [Chip],
    selected: bool,
    select: Option<MenuAction>,
    remove: Option<MenuAction>, // shown in the card's corner; locked while a removal runs
    controls: bool,
}

/// Draws a compact row, or the selected card with deletion, failure, settings and reload.
/// Narrow cards drop the icon tile and the indent, and wrap chips and controls onto new rows.
fn entry(content: &mut Content<'_, '_>, entry: Entry<'_>) -> Result<(), PaintError> {
    let view = content.view;
    let [left, right] = content.inset();
    let top = content.y;
    let canvas = &mut *content.canvas;
    let locked = view.shader_pack_removing.is_some();
    let pad = canvas.r(1.2);
    let button = canvas.r(4.4);
    let gap = canvas.r(0.8);
    let narrow = right - left < canvas.r(28.0);
    // Delete leaves the corner for the control rows when the name would have no room beside it.
    let corner = (!narrow).then_some(entry.remove).flatten();
    let select_right = if corner.is_some() {
        right - 2.0 * pad - button
    } else {
        right
    };
    let tile_left = left + canvas.r(4.8);
    let tile = if narrow { 0.0 } else { canvas.r(3.6) };
    let text_left = if narrow {
        tile_left
    } else {
        tile_left + tile + pad
    };
    let text_right = select_right.min(right - pad);
    let width = (text_right - text_left).max(1.0);
    let title_height = canvas
        .measure_height(entry.title, width, BODY)?
        .max(canvas.r(BODY.line));
    let caption_height = match entry.caption {
        Some(caption) => canvas
            .measure_height(caption, width, CAPTION)?
            .max(canvas.r(CAPTION.line)),
        None => 0.0,
    };
    let chip_gap = canvas.r(0.6);
    let widths = entry
        .chips
        .iter()
        .map(|chip| chip.width(canvas))
        .collect::<Result<Vec<_>, _>>()?;
    // A chip wider than the name column runs under the radio instead of past the card. The
    // radio then sits on the title's first line and the chips start below its rotated corners.
    let under_radio = widths.iter().any(|chip_width| *chip_width > width);
    let chips_left = if under_radio { left + pad } else { text_left };
    let chips_width = (text_right - chips_left).max(1.0);
    let radio_middle = canvas.r(BODY.line) * 0.5;
    let chip_top = if under_radio {
        (title_height + caption_height).max(radio_middle + canvas.r(super::super::radio::REACH))
    } else {
        title_height + caption_height
    };
    let mut chips = Vec::with_capacity(entry.chips.len());
    let (mut x, mut row, mut row_height) = (0.0, chip_top + canvas.r(0.4), 0.0_f32);
    for chip in entry.chips {
        let [chip_width, chip_height] = chip.size(canvas, chips_width)?;
        if x > 0.0 && x + chip_width > chips_width {
            (x, row, row_height) = (0.0, row + row_height + chip_gap, 0.0);
        }
        chips.push((*chip, [x, row, x + chip_width, row + chip_height]));
        x += chip_width + chip_gap;
        row_height = row_height.max(chip_height);
    }
    let stack = if chips.is_empty() {
        title_height + caption_height
    } else {
        row + row_height
    };
    let header = (stack + 2.0 * pad).max(tile + 2.0 * pad).max(canvas.r(4.8));

    // Controls sit under the name when they fit there, else span the card and wrap.
    let mut controls: Vec<(Control, f32)> = Vec::new();
    if entry.controls {
        if view.shader_options_available {
            controls.push((
                Control::Settings,
                canvas.measure("Settings", BODY)? + canvas.r(6.4),
            ));
        }
        controls.push((Control::Reload, button));
        if corner.is_none()
            && let Some(remove) = entry.remove
        {
            controls.push((Control::Delete(remove), button));
        }
    }
    let row_width = controls.iter().map(|(_, width)| width + gap).sum::<f32>() - gap;
    let inner = if right - pad - text_left >= row_width {
        [text_left, right - pad]
    } else {
        [left + pad, right - pad]
    };
    let inner_width = (inner[1] - inner[0]).max(1.0);
    let mut placed = Vec::with_capacity(controls.len());
    let (mut x, mut row) = (inner[0], 0.0);
    for (control, natural) in controls {
        let control_width = match control {
            Control::Settings if row_width > inner_width => inner_width,
            _ => natural.min(inner_width),
        };
        if x > inner[0] && x + control_width > inner[1] {
            (x, row) = (inner[0], row + button + gap);
        }
        placed.push((control, [x, row, x + control_width, row + button]));
        x += control_width + gap;
    }
    let controls_height = if placed.is_empty() {
        0.0
    } else {
        row + button + pad
    };
    let failure = entry
        .controls
        .then_some(view.renderer_failure.as_deref())
        .flatten();
    let failure_height = match failure {
        Some(text) => {
            canvas.measure_height(text, (inner_width - 2.0 * pad).max(1.0), CAPTION)?
                + 2.0 * canvas.r(0.8)
                + canvas.r(0.8)
        }
        None => 0.0,
    };
    let card = [
        left,
        top,
        right,
        top + header + failure_height + controls_height,
    ];

    let state = canvas.interaction(view, entry.select);
    if entry.selected {
        widgets::row(canvas, view, card, true, None)?;
        if state.focused {
            let ring = canvas.r(0.4);
            canvas.frame(
                [
                    card[0] - ring,
                    card[1] - ring,
                    card[2] + ring,
                    card[3] + ring,
                ],
                EDGE,
                theme::OUTLINE,
            )?;
        }
        if let Some(action) = entry.select {
            canvas.hit(action, [left, top, select_right, top + header])?;
        }
    } else {
        widgets::row(canvas, view, card, false, entry.select)?;
    }
    let middle = top + header * 0.5;
    let text_top = top + (header - stack) * 0.5;
    super::super::radio::draw(
        canvas,
        [
            left + canvas.r(2.4),
            if under_radio {
                text_top + radio_middle
            } else {
                middle
            },
        ],
        entry.selected,
        state,
        entry.select.is_some() || entry.selected,
    )?;
    if tile > 0.0 {
        let tile_bounds = [
            tile_left,
            middle - tile * 0.5,
            tile_left + tile,
            middle + tile * 0.5,
        ];
        canvas.fill(tile_bounds, theme::NEUTRAL80.fill)?;
        canvas.frame(tile_bounds, EDGE, theme::BORDER)?;
        super::super::icons::draw_centred(
            canvas,
            entry.icon,
            tile_bounds,
            if entry.selected { TEXT } else { TEXT_DIMMER },
        )?;
    }
    canvas.text(entry.title, [text_left, text_top], width, BODY, TEXT, false)?;
    if let Some(caption) = entry.caption {
        canvas.text(
            caption,
            [text_left, text_top + title_height],
            width,
            CAPTION,
            TEXT_DIMMER,
            false,
        )?;
    }
    for (chip, [x0, y0, x1, y1]) in chips {
        chip.draw(
            canvas,
            [
                chips_left + x0,
                text_top + y0,
                chips_left + x1,
                text_top + y1,
            ],
        )?;
    }
    if let Some(remove) = corner {
        widgets::icon_button(
            canvas,
            view,
            [
                right - pad - button,
                top + pad,
                right - pad,
                top + pad + button,
            ],
            Variant::Neutral,
            Icon::Trash,
            (!locked).then_some(remove),
        )?;
    }
    let mut y = top + header;
    if let Some(text) = failure {
        let bounds = [inner[0], y, inner[1], y + failure_height - canvas.r(0.8)];
        canvas.fill(bounds, theme::NEUTRAL80.fill)?;
        canvas.frame(bounds, EDGE, theme::BORDER)?;
        canvas.fill(
            [bounds[0], bounds[1], bounds[0] + canvas.r(0.4), bounds[3]],
            theme::DESTRUCTIVE_TINT,
        )?;
        canvas.text(
            text,
            [inner[0] + pad, y + canvas.r(0.8)],
            (inner_width - 2.0 * pad).max(1.0),
            CAPTION,
            TEXT_DIMMER,
            false,
        )?;
        y = bounds[3] + canvas.r(0.8);
    }
    for (control, at) in placed {
        let bounds = [at[0], y + at[1], at[2], y + at[3]];
        match control {
            Control::Settings => widgets::button(
                canvas,
                view,
                bounds,
                Variant::Secondary,
                "Settings",
                (!locked).then_some(MenuAction::ShaderOptions(Action::Open)),
            )?,
            Control::Reload => widgets::icon_button(
                canvas,
                view,
                bounds,
                Variant::Secondary,
                Icon::Reload,
                (!locked).then_some(MenuAction::ReloadShaders),
            )?,
            Control::Delete(remove) => widgets::icon_button(
                canvas,
                view,
                bounds,
                Variant::Neutral,
                Icon::Trash,
                (!locked).then_some(remove),
            )?,
        }
    }
    content.y = card[3] + content.canvas.r(0.8);
    Ok(())
}

/// A selected card's control, in reading and focus order.
#[derive(Clone, Copy)]
enum Control {
    Settings,
    Reload,
    Delete(MenuAction),
}

/// The informative note under the section heading.
fn banner(content: &mut Content<'_, '_>, text: &str) -> Result<(), PaintError> {
    let [left, right] = content.inset();
    let canvas = &mut *content.canvas;
    let pad = canvas.r(1.2);
    let bar = canvas.r(0.4);
    let [icon_width, _] = Icon::Info
        .texels()
        .map(|texels| canvas.r(texels as f32 * EDGE));
    // A narrow column keeps the accent bar but gives the icon's room to the text.
    let icon = right - left >= canvas.r(24.0);
    let text_left = left + bar + pad + if icon { icon_width + pad } else { 0.0 };
    let width = (right - pad - text_left).max(1.0);
    let height = canvas
        .measure_height(text, width, CAPTION)?
        .max(canvas.r(CAPTION.line))
        + 2.0 * canvas.r(0.8);
    let bounds = [left, content.y, right, content.y + height];
    canvas.fill(bounds, theme::NEUTRAL80.fill)?;
    canvas.frame(bounds, EDGE, theme::BORDER)?;
    canvas.fill(
        [left, bounds[1], left + bar, bounds[3]],
        theme::INFORMATIVE_TINT,
    )?;
    if icon {
        super::super::icons::draw_centred(
            canvas,
            Icon::Info,
            [
                left + bar + pad,
                bounds[1],
                left + bar + pad + icon_width,
                bounds[3],
            ],
            theme::INFORMATIVE_TINT,
        )?;
    }
    canvas.text(
        text,
        [text_left, bounds[1] + canvas.r(0.8)],
        width,
        CAPTION,
        TEXT_DIMMER,
        false,
    )?;
    content.y = bounds[3] + content.canvas.r(1.2);
    Ok(())
}

/// The dashed import target closing the list, with the folder shortcut where the host has one.
/// An empty library centres it as a larger first-use prompt.
fn drop_zone(content: &mut Content<'_, '_>) -> Result<(), PaintError> {
    let view = content.view;
    let [left, right] = content.inset();
    let canvas = &mut *content.canvas;
    let empty = view.shader_packs.is_empty();
    let pad = canvas.r(if empty && right - left >= canvas.r(32.0) {
        3.2
    } else {
        1.6
    });
    let title = "Drop a .zip or folder here";
    let caption = empty.then_some("Shader packs you add appear in this list.");
    let open = view
        .storage
        .opens_folders
        .then_some(MenuAction::SettingsStorage(StorageAction::OpenFolder(
            StorageFolder::ShaderPacks,
        )));
    let label = "Open folder";
    let full = (right - left - 2.0 * pad).max(1.0);
    let button = [
        (canvas.measure(label, BODY)? + canvas.r(4.8)).min(full),
        canvas.r(4.4),
    ];
    let inline =
        open.is_none() || (!empty && canvas.measure(title, BODY)? + pad + button[0] <= full);
    let width = if inline && open.is_some() {
        full - pad - button[0]
    } else {
        full
    };
    let title_height = canvas
        .measure_height(title, width, BODY)?
        .max(canvas.r(BODY.line));
    let caption_height = match caption {
        Some(caption) => canvas.measure_height(caption, width, CAPTION)?,
        None => 0.0,
    };
    let stack = title_height + caption_height;
    let gap = canvas.r(1.6);
    let body = match (open, inline) {
        (None, _) => stack,
        (Some(_), true) => stack.max(button[1]),
        (Some(_), false) => stack + gap + button[1],
    };
    let bounds = [left, content.y, right, content.y + body + 2.0 * pad];
    dashed_frame(canvas, bounds, theme::TEXT_DIMMEST)?;
    let text_top = if inline {
        (bounds[1] + bounds[3] - stack) * 0.5
    } else {
        bounds[1] + pad
    };
    let line = |canvas: &mut Canvas<'_>, text: &str, top: f32, style, color| {
        if empty {
            canvas.centered_wrapped_text(text, [left + pad, top], width, style, color)
        } else {
            canvas.text(text, [left + pad, top], width, style, color, false)
        }
    };
    line(
        canvas,
        title,
        text_top,
        BODY,
        if empty { TEXT } else { TEXT_DIMMER },
    )?;
    if let Some(caption) = caption {
        line(
            canvas,
            caption,
            text_top + title_height,
            CAPTION,
            TEXT_DIMMER,
        )?;
    }
    if let Some(open) = open {
        let x = if inline {
            right - pad - button[0]
        } else if empty {
            (left + right - button[0]) * 0.5
        } else {
            left + pad
        };
        let y = if inline {
            (bounds[1] + bounds[3] - button[1]) * 0.5
        } else {
            text_top + stack + gap
        };
        widgets::button(
            canvas,
            view,
            [x, y, x + button[0], y + button[1]],
            Variant::Secondary,
            label,
            Some(open),
        )?;
    }
    content.y = bounds[3] + content.canvas.r(2.4);
    Ok(())
}

/// A one-texel dashed outline.
fn dashed_frame(canvas: &mut Canvas<'_>, b: Bounds, color: theme::Rgba) -> Result<(), PaintError> {
    let edge = canvas.r(EDGE);
    let dash = canvas.r(1.2);
    let step = dash + canvas.r(0.8);
    let mut x = b[0];
    while x < b[2] {
        let end = (x + dash).min(b[2]);
        canvas.fill([x, b[1], end, b[1] + edge], color)?;
        canvas.fill([x, b[3] - edge, end, b[3]], color)?;
        x += step;
    }
    let mut y = b[1];
    while y < b[3] {
        let end = (y + dash).min(b[3]);
        canvas.fill([b[0], y, b[0] + edge, end], color)?;
        canvas.fill([b[2] - edge, y, b[2], end], color)?;
        y += step;
    }
    Ok(())
}

/// Draws the Video renderer choices with the shared segmented-control focus behavior.
fn picker<'a>(
    content: &mut Content<'_, '_>,
    title: &str,
    description: &str,
    choices: impl Iterator<Item = (RendererChoice, &'a str)> + Clone,
    selected: Option<RendererChoice>,
) -> Result<(), PaintError> {
    let view = content.view;
    let count = choices.clone().count();
    let [left, right] = content.inset();
    let vertical = content.column_width < content.canvas.r(15.0 * count as f32);
    let cell_width = (right - left) / if vertical { 1.0 } else { count as f32 };
    let height = choices.clone().try_fold(0.0_f32, |height, (_, label)| {
        widgets::choice_height(content.canvas, label, cell_width).map(|choice| height.max(choice))
    })?;
    let control_height = height * if vertical { count as f32 } else { 1.0 };
    let extra = control_height + content.canvas.r(0.4);
    let bounds = content.row(title, description, right - left, None, extra)?;
    content.texts(bounds, title, description, right - left, extra)?;
    let top = bounds[3] - control_height - content.canvas.r(1.2);
    let overlap = content.canvas.r(0.2);
    let cell = |index: usize| {
        if vertical {
            [
                left,
                top + index as f32 * height,
                right,
                top + (index + 1) as f32 * height,
            ]
        } else {
            [
                left + index as f32 * cell_width - overlap,
                top,
                left + (index + 1) as f32 * cell_width,
                top + height,
            ]
        }
    };
    for (index, (choice, label)) in choices.clone().enumerate() {
        widgets::choice(
            content.canvas,
            view,
            cell(index),
            label,
            selected == Some(choice),
            MenuAction::SelectRenderer(choice),
        )?;
    }
    for (index, (choice, _)) in choices.clone().enumerate() {
        widgets::choice_focus(
            content.canvas,
            view,
            cell(index),
            selected == Some(choice),
            MenuAction::SelectRenderer(choice),
        )?;
    }
    Ok(())
}
