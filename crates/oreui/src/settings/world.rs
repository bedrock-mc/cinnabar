//! The Game section for the host's own local world: vanilla's in-game world settings, locked
//! where vanilla locks them once the world exists.

use super::super::theme::BODY;
use super::super::widgets;
use super::{Content, controls};
use bridge::GameMode;
use launcher::local_worlds::in_world::{
    Action, GAME_MODE_TIMING, GAME_SECTION, Row, Timing, View, timing,
};
use launcher::local_worlds::options::{Control, WorldOption};
use launcher::local_worlds::world::{Difficulty, Generator};
use launcher::menu::{MenuAction, MenuField};
use ui_engine::engine::screen::PaintError;

/// Shown under settings the server reads only when the world starts.
const NEXT_START: &str = "Applies the next time the world starts.";
/// Shown once the running server failed to take a change this session.
const MISSED: &str =
    "The running world didn't take a change. It applies the next time the world starts.";
const FIELD_HEIGHT: f32 = 4.8;

pub(super) fn draw(content: &mut Content<'_, '_>) -> Result<(), PaintError> {
    let Some(world) = content.view.world_settings.clone() else {
        return Ok(());
    };
    for &row in GAME_SECTION {
        let title = content.word(row.label()).trim().to_owned();
        match row {
            Row::Heading(key) => {
                content.heading(key, "")?;
                if world.missed && row == GAME_SECTION[0] {
                    value(content, MISSED, "")?;
                }
                if row == GAME_SECTION[0]
                    && let Some(error) = &world.error
                {
                    value(
                        content,
                        &format!("Couldn't save the change: {error}"),
                        "Try the setting again.",
                    )?;
                }
            }
            Row::Name => value(content, &title, &world.world.name)?,
            Row::GameMode => {
                let current = world.world.game_mode;
                let modes = [GameMode::Survival, GameMode::Creative, GameMode::Adventure];
                let choices: Vec<_> = modes
                    .iter()
                    .map(|&mode| {
                        let label = content.word(game_mode_key(mode));
                        (label, mode == current, Action::GameMode(mode))
                    })
                    .collect();
                let note = if GAME_MODE_TIMING == Timing::NextStart {
                    NEXT_START
                } else {
                    ""
                };
                pick(content, &title, note, &choices)?;
            }
            Row::Hardcore => {
                let [left, right] = content.inset();
                let width = (right - left - content.canvas.r(7.6)).max(1.0);
                let b = content.row(&title, "", width, None, 0.0)?;
                content.texts(b, &title, "", width, 0.0)?;
                let at = [
                    right - content.canvas.r(6.0),
                    (b[1] + b[3]) * 0.5 - content.canvas.r(1.6),
                ];
                controls::fixed_toggle(content.canvas, at, false)?;
            }
            Row::Difficulty => {
                let current = world.world.difficulty;
                let choices: Vec<_> = [
                    Difficulty::Peaceful,
                    Difficulty::Easy,
                    Difficulty::Normal,
                    Difficulty::Hard,
                ]
                .iter()
                .map(|&difficulty| {
                    let label = content.word(difficulty_key(difficulty));
                    (label, difficulty == current, Action::Difficulty(difficulty))
                })
                .collect();
                pick(content, &title, "", &choices)?;
            }
            Row::WorldType => {
                let kind = content.word(match world.world.generator {
                    Generator::Normal => "generator.infinite",
                    Generator::Flat => "generator.flat",
                });
                value(content, &title, &kind)?;
            }
            Row::Seed => value(content, &title, &world.world.seed.to_string())?,
            // Vanilla offers default permissions only while the default game mode is not adventure.
            Row::Option(WorldOption::PlayerPermissions, ..)
                if world.world.game_mode == GameMode::Adventure => {}
            Row::Option(id, _, description) => {
                let description = content.word(description).trim().to_owned();
                option(content, &world, id, &title, &description)?;
            }
        }
    }
    Ok(())
}

fn settings(action: Action) -> MenuAction {
    MenuAction::WorldSettings(action)
}

/// One catalog option's control, disabled where it cannot change while the world runs.
fn option(
    content: &mut Content<'_, '_>,
    world: &View,
    id: WorldOption,
    title: &str,
    description: &str,
) -> Result<(), PaintError> {
    let enabled = world.editable(id);
    let description = if timing(id) == Timing::NextStart && description.is_empty() {
        NEXT_START
    } else {
        description
    };
    let current = world.world.options.get(id);
    match id.definition().control {
        Control::Toggle => content.boolean(
            title,
            description,
            current != 0,
            settings(Action::Option(id, 1 - current)),
            enabled,
        ),
        Control::Choice(choices) => {
            let choices: Vec<_> = choices
                .iter()
                .map(|choice| {
                    let action = Action::Option(id, choice.value);
                    (choice.label.to_owned(), choice.value == current, action)
                })
                .collect();
            pick(content, title, description, &choices)
        }
        Control::Number { .. } if !enabled => {
            let shown = match id {
                WorldOption::SimulationDistance => content
                    .word("createWorldScreen.serverSimDistanceFormat")
                    .replace("%s", &current.to_string()),
                _ => current.to_string(),
            };
            value(content, title, &shown)
        }
        Control::Number { min, max } => {
            let text = world.numeric_text(id);
            let valid = text.parse().is_ok_and(|value| id.accepts(value));
            let hint;
            let description = if valid {
                description
            } else {
                hint = format!("Enter a number from {min} to {max}.");
                &hint
            };
            number(content, title, description, id, &text)
        }
    }
}

/// A row whose locked value reads as its description.
fn value(content: &mut Content<'_, '_>, title: &str, value: &str) -> Result<(), PaintError> {
    let [left, right] = content.inset();
    let b = content.row(title, value, right - left, None, 0.0)?;
    content.texts(b, title, value, right - left, 0.0)
}

/// A row of side-by-side choices, the current one selected.
fn pick(
    content: &mut Content<'_, '_>,
    title: &str,
    description: &str,
    choices: &[(String, bool, Action)],
) -> Result<(), PaintError> {
    let [left, right] = content.inset();
    let cell = (right - left) / choices.len().max(1) as f32;
    let height = choices.iter().try_fold(0.0_f32, |height, (label, ..)| {
        widgets::choice_height(content.canvas, label, cell).map(|choice| height.max(choice))
    })?;
    let extra = height + content.canvas.r(0.4);
    let b = content.row(title, description, right - left, None, extra)?;
    content.texts(b, title, description, right - left, extra)?;
    let top = b[3] - height - content.canvas.r(1.2);
    for (index, (label, on, action)) in choices.iter().enumerate() {
        let bounds = [
            left + index as f32 * cell - content.canvas.r(0.2),
            top,
            left + (index + 1) as f32 * cell,
            top + height,
        ];
        widgets::choice(
            content.canvas,
            content.view,
            bounds,
            label,
            *on,
            settings(*action),
        )?;
        widgets::choice_focus(content.canvas, content.view, bounds, *on, settings(*action))?;
    }
    Ok(())
}

/// A numeric option's text box; typing a valid number applies it.
fn number(
    content: &mut Content<'_, '_>,
    title: &str,
    description: &str,
    id: WorldOption,
    text: &str,
) -> Result<(), PaintError> {
    let [left, right] = content.inset();
    let height = content.canvas.r(FIELD_HEIGHT);
    let action = settings(Action::OptionField(id));
    let b = content.row(
        title,
        description,
        right - left,
        Some(action),
        height + content.canvas.r(0.4),
    )?;
    content.texts(
        b,
        title,
        description,
        right - left,
        height + content.canvas.r(0.4),
    )?;
    let top = b[3] - height - content.canvas.r(1.2);
    let width = (content.canvas.measure("00000", BODY)? + content.canvas.r(4.8)).min(right - left);
    widgets::text_field_on_panel(
        content.canvas,
        content.view,
        [left, top, left + width, top + height],
        text,
        "",
        content.view.field == Some(MenuField::WorldOption(id)),
        Some(action),
    )
}

const fn game_mode_key(mode: GameMode) -> &'static str {
    match mode {
        GameMode::Survival => "createWorldScreen.gameMode.survival",
        GameMode::Creative => "createWorldScreen.gameMode.creative",
        GameMode::Adventure => "createWorldScreen.gameMode.adventure",
    }
}

const fn difficulty_key(difficulty: Difficulty) -> &'static str {
    match difficulty {
        Difficulty::Peaceful => "options.difficulty.peaceful",
        Difficulty::Easy => "options.difficulty.easy",
        Difficulty::Normal => "options.difficulty.normal",
        Difficulty::Hard => "options.difficulty.hard",
    }
}

#[cfg(test)]
mod tests;
