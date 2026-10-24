//! Compact permission summaries, explicit consent and optional customization.

use super::{Content, actions, command};
use crate::{
    settings::resources::caption,
    theme,
    widgets::{self, Variant},
};
use launcher::{
    menu::MenuAction,
    mods::{Action, InstalledMod, Snapshot},
};
use ui_engine::engine::screen::PaintError;

/// Draws the summary and either consent or customization beneath the card's actions.
pub(super) fn draw(
    content: &mut Content<'_, '_>,
    snapshot: &Snapshot,
    item: &InstalledMod,
    active: bool,
    available: bool,
) -> Result<(), PaintError> {
    content.y += content.canvas.r(1.2);
    let (allowed, requested) = item.requested_counts();
    let sensitive: Vec<_> = item
        .permissions
        .iter()
        .filter(|permission| permission.enabled && permission.sensitive)
        .map(|permission| permission.name.as_str())
        .collect();
    let mut summary = format!("{allowed} of {requested} permissions");
    if !sensitive.is_empty() {
        summary.push_str(" · ");
        summary.push_str(&sensitive.join(", "));
    }
    let [left, right] = content.inset();
    content.canvas.text_line(
        &summary,
        [left, content.y],
        right - left,
        theme::CAPTION,
        theme::TEXT_DIMMER,
    )?;
    content.y += content.canvas.r(theme::CAPTION.line + 0.8);
    if snapshot.customizing == Some(item.id) {
        customize(content, snapshot, item, available)
    } else if active
        && (snapshot.consent == Some(item.id)
            || allowed < requested
            || !item.declares_permissions())
    {
        consent(content, item, available)
    } else {
        actions(
            content,
            &[(
                "Customize",
                Variant::Secondary,
                available.then_some(command(Action::Customize(item.id))),
            )],
        )?;
        content.y += content.canvas.r(0.8);
        Ok(())
    }
}

/// Shows only declared requests and offers explicit one-click consent when they exist.
fn consent(
    content: &mut Content<'_, '_>,
    item: &InstalledMod,
    available: bool,
) -> Result<(), PaintError> {
    if item.declares_permissions() {
        caption(content, "This mod requests:")?;
        chips(content, item)?;
    } else {
        caption(content, "This mod doesn't declare its permissions")?;
    }
    let mut buttons = Vec::new();
    if item.declares_permissions() {
        buttons.push((
            "Allow requested",
            Variant::Primary,
            available.then_some(command(Action::AllowRequested(item.id))),
        ));
    }
    buttons.push((
        "Customize",
        Variant::Secondary,
        available.then_some(command(Action::Customize(item.id))),
    ));
    actions(content, &buttons)?;
    content.y += content.canvas.r(0.8);
    Ok(())
}

/// Wraps the existing tags into rows without treating unrequested capabilities as requests.
fn chips(content: &mut Content<'_, '_>, item: &InstalledMod) -> Result<(), PaintError> {
    let [left, right] = content.inset();
    let gap = content.canvas.r(0.8);
    let mut x = left;
    let mut any = false;
    for index in item.permission_order(false) {
        let label = item.permissions[index].label();
        let width = content.canvas.measure(&label, theme::BODY)? + gap;
        if x > left && x + width > right {
            x = left;
            content.y += content.canvas.r(2.8);
        }
        let fill = content.canvas.role(theme::NEUTRAL).fill;
        x = widgets::tag(content.canvas, &label, [x, content.y], fill, theme::TEXT)? + gap;
        any = true;
    }
    if any {
        content.y += content.canvas.r(3.2);
    } else {
        caption(content, "No permissions requested.")?;
    }
    Ok(())
}

/// Keeps batch actions, a requested-first grid and the full-list disclosure together.
fn customize(
    content: &mut Content<'_, '_>,
    snapshot: &Snapshot,
    item: &InstalledMod,
    available: bool,
) -> Result<(), PaintError> {
    let mut buttons = Vec::new();
    if item.declares_permissions() {
        buttons.push((
            "Allow requested",
            Variant::Primary,
            available.then_some(command(Action::AllowRequested(item.id))),
        ));
    }
    buttons.push((
        "Revoke all",
        Variant::Destructive,
        available.then_some(command(Action::RevokeAll(item.id))),
    ));
    buttons.push((
        "Done",
        Variant::Secondary,
        available.then_some(command(Action::Customize(item.id))),
    ));
    actions(content, &buttons)?;
    content.y += content.canvas.r(1.2);
    let indices = item.permission_order(snapshot.show_all_permissions);
    if indices.is_empty() {
        caption(
            content,
            "No declared requests. Show all permissions to choose.",
        )?;
    }
    grid(content, item, &indices, available)?;
    let description = [content.view.hovered, content.view.focused_action]
        .into_iter()
        .flatten()
        .find_map(|action| match action {
            MenuAction::Mods(Action::Permission(id, index, _))
                if id == item.id && indices.contains(&index) =>
            {
                item.permissions.get(index)
            }
            _ => None,
        });
    caption(
        content,
        description.map_or(
            "Hover or focus a permission for its description.",
            |permission| permission.description.as_str(),
        ),
    )?;
    let [left, right] = content.inset();
    let y = content.y;
    let action = command(Action::ShowAllPermissions(item.id));
    super::super::controls::toggle_control(
        content.canvas,
        content.view,
        [left, y],
        snapshot.show_all_permissions,
        action,
        available,
    )?;
    content.canvas.text_line_vertically_centred(
        "Show all permissions",
        [
            left + content.canvas.r(8.0),
            y,
            right,
            y + content.canvas.r(3.6),
        ],
        theme::BODY,
        theme::TEXT,
    )?;
    if available {
        content
            .canvas
            .hit(action, [left, y, right, y + content.canvas.r(3.6)])?;
    }
    content.y += content.canvas.r(4.4);
    Ok(())
}

/// Uses two columns when the short labels fit and one column at narrower widths.
fn grid(
    content: &mut Content<'_, '_>,
    item: &InstalledMod,
    indices: &[usize],
    available: bool,
) -> Result<(), PaintError> {
    let [left, right] = content.inset();
    let gap = content.canvas.r(0.8);
    let width = (right - left).max(1.0);
    let minimum = indices.iter().try_fold(0.0_f32, |minimum, index| {
        content
            .canvas
            .measure(&item.permissions[*index].label(), theme::BODY)
            .map(|label| minimum.max(label + content.canvas.r(9.6)))
    })?;
    let columns = if (width - gap) * 0.5 >= minimum { 2 } else { 1 };
    let cell = (width - gap * (columns - 1) as f32) / columns as f32;
    let height = content.canvas.r(4.4);
    for (position, index) in indices.iter().enumerate() {
        let permission = &item.permissions[*index];
        let x = left + (position % columns) as f32 * (cell + gap);
        let y = content.y + (position / columns) as f32 * height;
        let action = command(Action::Permission(item.id, *index, !permission.enabled));
        let bounds = [x, y, x + cell, y + content.canvas.r(3.6)];
        widgets::row(
            content.canvas,
            content.view,
            bounds,
            false,
            available.then_some(action),
        )?;
        let at = [x + content.canvas.r(0.4), y + content.canvas.r(0.2)];
        super::super::controls::toggle_control(
            content.canvas,
            content.view,
            at,
            permission.enabled,
            action,
            available,
        )?;
        content.canvas.text_line_vertically_centred(
            &permission.label(),
            [
                x + content.canvas.r(7.6),
                y,
                bounds[2] - content.canvas.r(0.4),
                bounds[3],
            ],
            theme::BODY,
            theme::TEXT,
        )?;
    }
    content.y += indices.len().div_ceil(columns) as f32 * height;
    Ok(())
}
