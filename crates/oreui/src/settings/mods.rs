//! The Mods tab: the resource-pack page's toolbar, groups and cards over the mod library.

use super::super::theme;
use super::super::transitions::resources::{Group, Listing, Mods};
use super::super::widgets::Variant;
use super::Content;
mod permissions;
mod update;
use super::resources::{
    Face, accordion, actions, caption, card_with_summary, cards, empty, note, reveal,
};
use launcher::menu::{MenuAction, file_size};
use launcher::mods::{Action, InstalledMod, Snapshot, Status};
use ui_engine::engine::screen::PaintError;

#[cfg(test)]
mod tests;

/// Wraps a card action for the shared Settings input router.
fn command(action: Action) -> MenuAction {
    MenuAction::Mods(action)
}

/// Draws the ordered Active and available mod groups with retained card motion.
pub(super) fn draw(content: &mut Content<'_, '_>) -> Result<(), PaintError> {
    let mut motion = content
        .canvas
        .transitions
        .as_deref_mut()
        .map(|transitions| std::mem::take(&mut transitions.mods))
        .unwrap_or_default();
    let animated = content
        .canvas
        .transitions
        .as_deref()
        .is_some_and(|t| t.motion.enabled());
    let snapshot = content.view.mods.clone();
    let (active, available) = snapshot.library.groups();
    let position = |list: &[InstalledMod], id: u64| list.iter().position(|item| item.id == id);
    let details = snapshot.details_expanded.and_then(|id| {
        position(&active, id)
            .map(|index| (true, index))
            .or_else(|| position(&available, id).map(|index| (false, index)))
    });
    motion.sync_listing(
        Listing {
            active: &active,
            available: &available,
            expanded: [snapshot.active_expanded, snapshot.available_expanded],
            details,
            pending: false,
        },
        content.canvas.seconds,
        animated,
    );
    let result = draw_sections(content, &snapshot, &motion);
    if let Some(transitions) = content.canvas.transitions.as_deref_mut() {
        transitions.mods = motion;
    }
    result
}

/// Draws page actions and both groups from one immutable library snapshot.
fn draw_sections(
    content: &mut Content<'_, '_>,
    snapshot: &Snapshot,
    motion: &Mods,
) -> Result<(), PaintError> {
    let library = &snapshot.library;
    content.heading("Mods", "Add features to your game with WebAssembly mods.")?;
    actions(
        content,
        &[
            (
                "Import mod…",
                Variant::Secondary,
                (!library.busy).then_some(command(Action::Import)),
            ),
            (
                "Reload all",
                Variant::Secondary,
                (!library.busy && !library.active.is_empty()).then_some(command(Action::ReloadAll)),
            ),
        ],
    )?;
    content.y += content.canvas.r(1.2);
    if !library.message.is_empty() {
        note(content, &library.message, theme::NEUTRAL80.fill)?;
    }
    let enabled = library.active.len();
    accordion(
        content,
        &format!("Active mods ({enabled})"),
        motion.expanded[0],
        command(Action::ToggleActive),
    )?;
    reveal(
        content,
        motion.expanded[0],
        snapshot.active_expanded,
        |content| {
            cards(content, motion, Group::Active, |content, card| {
                mod_card(
                    content,
                    snapshot,
                    &card.item,
                    true,
                    card.details,
                    card.present,
                )
            })?;
            caption(
                content,
                "Higher mods take priority. Reorder Active mods to choose which wins.",
            )
        },
    )?;
    content.y += content.canvas.r(1.2);
    accordion(
        content,
        &format!("My mods ({})", library.installed.len() - enabled),
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
                    if library.installed.is_empty() {
                        "No mods installed"
                    } else {
                        "No other mods"
                    },
                    "Import a .wasm mod component to add it here.",
                )
            })?;
            cards(content, motion, Group::Available, |content, card| {
                mod_card(
                    content,
                    snapshot,
                    &card.item,
                    false,
                    card.details,
                    card.present,
                )
            })
        },
    )?;
    content.y += content.canvas.r(2.4);
    Ok(())
}

/// Draws declared metadata, identity-scoped controls, reload outcomes and generated permission
/// toggles.
fn mod_card(
    content: &mut Content<'_, '_>,
    snapshot: &Snapshot,
    item: &InstalledMod,
    active: bool,
    details: f32,
    interactive: bool,
) -> Result<(), PaintError> {
    let library = &snapshot.library;
    let status = library
        .active
        .iter()
        .find(|entry| entry.id == item.id)
        .map(|entry| &entry.status);
    let failure = status.and_then(|status| match status {
        Status::Failed(message) | Status::ReloadFailed(message) => Some(message.as_str()),
        _ => None,
    });
    let state = match (active, status) {
        (false, _) => "Ready to enable",
        (true, Some(Status::Running)) => "Running",
        (true, Some(Status::NextJoin)) => "Ready for your next server join",
        (true, Some(Status::ReloadFailed(_))) => "Reload failed · previous instance running",
        (true, Some(Status::Failed(_))) => "Failed",
        (true, Some(Status::Starting) | None) => "Starting…",
    };
    let metadata = &item.metadata;
    let subtitle = match &metadata.version {
        Some(version) => {
            let version = version.strip_prefix(['v', 'V']).unwrap_or(version);
            format!("{state} · v{version}")
        }
        None => state.to_owned(),
    };
    let icon = item.icon_key();
    let face = Face {
        icon: icon.as_deref(),
        title: item.name(),
        subtitle: &subtitle,
        expanded: snapshot.details_expanded == Some(item.id),
        toggle: command(Action::ReadMore(item.id)),
        details,
        interactive,
    };
    card_with_summary(
        content,
        face,
        |content| {
            let available = interactive && !library.busy;
            let update = library.updates.iter().find(|update| update.id == item.id);
            let progress = library
                .update_progress
                .iter()
                .find(|progress| progress.id == item.id);
            if let Some(progress) = progress {
                update::draw(content, progress)?;
            } else if let Some(update) = update {
                caption(content, &format!("Update available · {}", update.version))?;
            }
            let toggle = if active {
                (
                    "Disable",
                    Variant::Secondary,
                    available.then_some(command(Action::Disable(item.id))),
                )
            } else {
                (
                    "Enable",
                    Variant::Primary,
                    available.then_some(command(Action::Enable(item.id))),
                )
            };
            let mut buttons = vec![toggle];
            if update.is_some() || progress.is_some() {
                let installable = item.metadata.update_url.is_some();
                let updating = progress.is_some();
                let label = if let Some(progress) = progress {
                    progress.phase.label()
                } else if installable {
                    "Update"
                } else {
                    "View release"
                };
                let action = if installable {
                    Action::Update(item.id)
                } else {
                    Action::OpenUpdate(item.id)
                };
                buttons.push((
                    label,
                    if installable {
                        Variant::Primary
                    } else {
                        Variant::Secondary
                    },
                    (available && !updating).then_some(command(action)),
                ));
            }
            if active {
                buttons.push((
                    "Reload",
                    Variant::Secondary,
                    available.then_some(command(Action::Reload(item.id))),
                ));
                let position = library.active.iter().position(|entry| entry.id == item.id);
                if position.is_some_and(|position| position > 0) {
                    buttons.push((
                        "Move up",
                        Variant::Neutral,
                        available.then_some(command(Action::MoveUp(item.id))),
                    ));
                }
                if position.is_some_and(|position| position + 1 < library.active.len()) {
                    buttons.push((
                        "Move down",
                        Variant::Neutral,
                        available.then_some(command(Action::MoveDown(item.id))),
                    ));
                }
            }
            buttons.push((
                "Remove",
                Variant::Destructive,
                available.then_some(command(Action::Remove(item.id))),
            ));
            actions(content, &buttons)?;
            permissions::draw(content, snapshot, item, active, available)
        },
        |content| {
            if let Some(description) = &metadata.description {
                caption(content, description)?;
            }
            if let Some(authors) = &metadata.authors {
                caption(content, &format!("By {authors}"))?;
            }
            let mut source = format!("{} · {}", item.file, file_size(item.bytes));
            if let Some(licenses) = &metadata.licenses {
                source = format!("{source} · {licenses}");
            }
            caption(content, &source)?;
            if let Some(message) = failure {
                caption(content, message)?;
            }
            Ok(())
        },
    )
}
