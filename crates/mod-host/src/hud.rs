//! Transactional cosmetic presentation, with no input or world authority.

use super::{MAX_IMPORT_WRITES, State};
use anyhow::{Result, bail};
use ui::mod_hud::{Crosshair, EditorResult, Hud, MAX_CROSSHAIR_BYTES, MAX_HUD_BYTES};
#[path = "hud/bounds.rs"]
pub(super) mod bounds;
#[path = "hud/clock.rs"]
mod clock;
#[path = "hud/compass.rs"]
mod compass;
#[path = "hud/sources.rs"]
pub(super) mod sources;
#[cfg(test)]
#[path = "hud/sources_tests.rs"]
mod sources_tests;

#[derive(Default)]
pub(super) struct HudState {
    pub revision: u64,
    pub content: Option<Hud>,
    pub crosshair: Option<Crosshair>,
    pending_content: Option<Option<Hud>>,
    pending_crosshair: Option<Option<Crosshair>>,
    pub editor_request: Option<Hud>,
    pub editor_result: Option<EditorResult>,
    pending_editor: Option<Hud>,
    result_read: bool,
    pub local_messages: Vec<String>,
    pending_messages: Vec<String>,
    source_snapshot: Option<crate::PlayerStateSnapshot>,
    source_sidebar: Option<crate::SidebarSnapshot>,
    sources_dirty: bool,
    pub(super) resolved: Option<Hud>,
    source_actors: Vec<crate::ObservedActor>,
    source_world: Option<crate::WorldObservation>,
    source_motion: Option<crate::MotionObservation>,
    source_clock: Option<clock::LocalMinute>,
    source_totem_uses: Option<u32>,
}
impl HudState {
    /// Identifies changed committed output across component replacement and reload.
    pub(super) fn changed(&mut self) {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        self.revision = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn commit(&mut self) {
        let room = 4usize.saturating_sub(self.local_messages.len());
        self.local_messages
            .extend(self.pending_messages.drain(..).take(room));
        if self.result_read {
            self.editor_result = None;
            self.result_read = false;
        }
        if let Some(content) = self.pending_content.take()
            && self.content != content
        {
            self.sources_dirty = true;
            self.content = content;
            self.changed();
        }
        if let Some(crosshair) = self.pending_crosshair.take() {
            self.crosshair = crosshair;
        }
        if let Some(preview) = self.pending_editor.take() {
            self.editor_request = Some(preview);
            self.editor_result = None;
        }
    }
}
/// Stages validated previews only while a granted panel owns focused input.
pub(super) fn open_editor(state: &mut State, json: String) -> Result<Result<(), String>> {
    if let Err(error) = admit(state)? {
        return Ok(Err(error));
    }
    if !state.grants.controls {
        return Ok(Err("controls capability denied".into()));
    }
    if !state.controls.frame.focused || !state.controls.frame.panel_open {
        return Ok(Err("HUD editor requires an input-owning panel".into()));
    }
    if json.len() > MAX_HUD_BYTES {
        return Ok(Err("HUD preview exceeds its byte limit".into()));
    }
    let mut preview: Hud = match serde_json::from_str(&json) {
        Ok(preview) => preview,
        Err(error) => return Ok(Err(format!("invalid HUD preview: {error}"))),
    };
    if let Err(error) = preview.validate() {
        return Ok(Err(error));
    }
    if let Err(error) = sources::admit(state, &preview) {
        return Ok(Err(error));
    }
    let minute = clock::observe(&preview);
    clock::hydrate(&mut preview, minute);
    sources::hydrate_totems(
        &mut preview,
        state.player_state.observations.local_totem_uses,
    );
    sources::hydrate_preview(
        &mut preview,
        state.player_state.snapshot.as_ref(),
        state.player_state.sidebar.as_ref(),
    );
    sources::hydrate_health(
        &mut preview,
        &state.player_state.observations.actors,
        state.player_state.world.as_ref(),
    );
    compass::hydrate(
        &mut preview,
        state.player_state.world.as_ref(),
        state.player_state.observations.motion.as_ref(),
    );
    state.hud.pending_editor = Some(preview);
    Ok(Ok(()))
}

/// Returns the same host result until a successful callback consumes it.
pub(super) fn read_editor_result(
    state: &mut State,
) -> Result<Result<Option<super::cinnabar::extension::hud::EditorResult>, String>> {
    if let Err(error) = admit(state)? {
        return Ok(Err(error));
    }
    if !state.grants.controls {
        return Ok(Err("controls capability denied".into()));
    }
    state.hud.result_read = true;
    Ok(Ok(state.hud.editor_result.as_ref().map(|result| {
        use super::cinnabar::extension::hud::{EditorResult, Placement, Point};
        EditorResult {
            saved: result.saved,
            reset: result.reset,
            placements: result
                .placements
                .iter()
                .map(|p| Placement {
                    id: p.id.clone(),
                    position: p.position.map(|[x, y]| Point { x, y }),
                })
                .collect(),
        }
    })))
}
/// Returns the same host result until a successful callback consumes it.
pub(super) fn read_editor_layout(
    state: &mut State,
) -> Result<Result<Option<super::cinnabar::extension::hud::LayoutResult>, String>> {
    if let Err(error) = admit(state)? {
        return Ok(Err(error));
    }
    if !state.grants.controls {
        return Ok(Err("controls capability denied".into()));
    }
    state.hud.result_read = true;
    Ok(Ok(state.hud.editor_result.as_ref().map(|result| {
        use super::cinnabar::extension::hud::{LayoutPlacement, LayoutResult, Point};
        LayoutResult {
            saved: result.saved,
            reset: result.reset,
            placements: result
                .placements
                .iter()
                .map(|p| LayoutPlacement {
                    id: p.id.clone(),
                    scale: p.scale,
                    position: p.position.map(|[x, y]| Point { x, y }),
                })
                .collect(),
        }
    })))
}
fn admit(state: &mut State) -> Result<Result<(), String>> {
    state.writes += 1;
    if state.writes > MAX_IMPORT_WRITES {
        bail!("HUD import budget exhausted");
    }
    Ok(if state.grants.hud {
        Ok(())
    } else {
        Err("HUD capability denied".into())
    })
}
pub(super) fn set_content(state: &mut State, json: String) -> Result<Result<(), String>> {
    if let Err(error) = admit(state)? {
        return Ok(Err(error));
    }
    if json.len() > MAX_HUD_BYTES {
        return Ok(Err("HUD exceeds its byte limit".into()));
    }
    let content = if json.is_empty() {
        None
    } else {
        let hud: Hud = match serde_json::from_str(&json) {
            Ok(hud) => hud,
            Err(error) => return Ok(Err(format!("invalid HUD: {error}"))),
        };
        if let Err(error) = hud.validate() {
            return Ok(Err(error));
        }
        Some(hud)
    };
    Ok(stage_content(state, content))
}

/// Replaces individual cards while keeping unrelated geometry and presentation retained.
pub(super) fn update_content(state: &mut State, json: String) -> Result<Result<(), String>> {
    if let Err(error) = admit(state)? {
        return Ok(Err(error));
    }
    if json.len() > MAX_HUD_BYTES {
        return Ok(Err("HUD update exceeds its byte limit".into()));
    }
    let update: ui::mod_hud::HudUpdate = match serde_json::from_str(&json) {
        Ok(update) => update,
        Err(error) => return Ok(Err(format!("invalid HUD update: {error}"))),
    };
    let existing = state
        .hud
        .pending_content
        .as_ref()
        .unwrap_or(&state.hud.content);
    let content = match update.apply(existing.as_ref()) {
        Ok(content) => content,
        Err(error) => return Ok(Err(error)),
    };
    Ok(stage_content(state, Some(content)))
}

/// Stages admitted content and card claims; editor settings alone do not retain a HUD.
fn stage_content(state: &mut State, content: Option<Hud>) -> Result<(), String> {
    let content = content.filter(|hud| {
        let Hud {
            cards,
            actor_style,
            actor_health,
            actor_bounds,
            hunger_overlay,
            hide_effect_icons,
            hide_scoreboard,
            world_labels,
            chat_actions,
            autosave: _,
            resizable: _,
            hide_editor_labels: _,
            surface: _,
            editor_style: _,
        } = hud;
        actor_style.is_some()
            || actor_health.is_some()
            || actor_bounds.is_some()
            || hunger_overlay.is_some()
            || !cards.is_empty()
            || *hide_effect_icons
            || *hide_scoreboard
            || !world_labels.is_empty()
            || !chat_actions.is_empty()
    });
    if let Some(content) = &content {
        sources::admit(state, content)?;
    }
    if let Some(content) = &content
        && let Some(claims) = &state.exclusive.hud
        && !claims.claim(
            state.exclusive.owner,
            content.cards.iter().map(|card| card.id.clone()),
        )
    {
        return Err(state.exclusive.refuse());
    }
    state.hud.pending_content = Some(content);
    Ok(())
}
pub(super) fn set_crosshair(state: &mut State, json: String) -> Result<Result<(), String>> {
    if let Err(error) = admit(state)? {
        return Ok(Err(error));
    }
    if json.len() > MAX_CROSSHAIR_BYTES {
        return Ok(Err("crosshair exceeds its byte limit".into()));
    }
    let crosshair = if json.is_empty() {
        None
    } else {
        let crosshair: Crosshair = match serde_json::from_str(&json) {
            Ok(spec) => spec,
            Err(error) => return Ok(Err(format!("invalid crosshair: {error}"))),
        };
        if let Err(error) = crosshair.validate() {
            return Ok(Err(error));
        }
        Some(crosshair)
    };
    state.hud.pending_crosshair = Some(crosshair);
    Ok(Ok(()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ModGrants;

    /// Empty edits release committed or staged content only after a successful callback.
    #[test]
    fn empty_card_updates_release_retained_content_on_commit() {
        let empty = r#"{"cards":[]}"#;
        let card = r#"{"cards":[{"id":"fps","rows":[]}]}"#;
        let remove = r#"{"remove":["fps"]}"#;
        let cases: &[(&str, &[&str])] = &[
            (card, &[remove]),
            (empty, &[card, remove]),
            (empty, &["{}"]),
            (empty, &[remove]),
            (
                r#"{"cards":[],"hide_scoreboard":true}"#,
                &[r#"{"presentation":{"cards":[]}}"#],
            ),
            (
                r#"{"cards":[{"id":"fps","rows":[]}],"hide_effect_icons":true}"#,
                &[r#"{"remove":["fps"],"presentation":{"cards":[]}}"#],
            ),
            (
                empty,
                &[r#"{"presentation":{"cards":[],"autosave":true,"resizable":true}}"#],
            ),
        ];
        for &(initial, updates) in cases {
            let mut state = State::new(
                ModGrants {
                    hud: true,
                    ..Default::default()
                },
                String::new(),
                Default::default(),
            );
            set_content(&mut state, initial.into()).unwrap().unwrap();
            state.hud.commit();
            let previous = state.hud.content.clone();
            for update in updates {
                update_content(&mut state, (*update).into())
                    .unwrap()
                    .unwrap();
                assert_eq!(state.hud.content, previous);
            }
            state.hud.commit();
            assert!(
                state.hud.content.is_none(),
                "{initial} followed by {updates:?}"
            );
        }
    }

    /// Removing the last card keeps every independently published presentation policy.
    #[test]
    fn card_updates_retain_non_card_presentation() {
        for presentation in [
            r#"{"cards":[],"actor_style":{"hurt_color":[1,0,0,1]}}"#,
            r#"{"cards":[],"actor_health":{"players":true,"mobs":false,"range":32}}"#,
            r#"{"cards":[],"actor_bounds":{"players":true,"mobs":false,"range":32,"color":[1,1,1,1]}}"#,
            r#"{"cards":[],"hunger_overlay":{"saturation":true}}"#,
            r#"{"cards":[],"hide_effect_icons":true}"#,
            r#"{"cards":[],"hide_scoreboard":true}"#,
            r#"{"cards":[],"world_labels":[{"text":"Waypoint"}]}"#,
            r#"{"cards":[],"chat_actions":[{"text":"Help","action":"help"}]}"#,
        ] {
            let mut state = State::new(
                ModGrants {
                    hud: true,
                    render: true,
                    players: true,
                    ..Default::default()
                },
                String::new(),
                Default::default(),
            );
            let expected: Hud = serde_json::from_str(presentation).unwrap();
            let mut initial = expected.clone();
            initial.cards.push(ui::mod_hud::Card {
                id: "fps".into(),
                ..Default::default()
            });
            set_content(&mut state, serde_json::to_string(&initial).unwrap())
                .unwrap()
                .unwrap();
            state.hud.commit();
            update_content(&mut state, r#"{"remove":["fps"]}"#.into())
                .unwrap()
                .unwrap();
            state.hud.commit();
            assert_eq!(state.hud.content, Some(expected));
        }
    }

    /// Retained edits compose in one callback and publish only on successful commit.
    #[test]
    fn card_updates_are_granted_bounded_and_transactional() {
        let mut state = State::new(
            ModGrants {
                hud: true,
                ..Default::default()
            },
            String::new(),
            Default::default(),
        );
        let first = r#"{"cards":[{"id":"fps","rows":[]}]}"#;
        let second = r#"{"cards":[{"id":"cps","rows":[]}]}"#;
        assert!(update_content(&mut state, first.into()).unwrap().is_ok());
        assert!(state.hud.content.is_none());
        assert!(update_content(&mut state, second.into()).unwrap().is_ok());
        assert!(
            update_content(
                &mut state,
                r#"{"cards":[{"id":"bad","width":0,"rows":[]}]}"#.into()
            )
            .unwrap()
            .is_err()
        );
        state.hud.commit();
        assert_eq!(state.hud.content.as_ref().unwrap().cards.len(), 2);
        state.grants.hud = false;
        assert!(
            update_content(&mut state, r#"{"remove":["fps"]}"#.into())
                .unwrap()
                .is_err()
        );
        state.hud.commit();
        assert_eq!(state.hud.content.as_ref().unwrap().cards.len(), 2);
    }
    #[test]
    fn effect_icon_replacement_commits_without_cards_and_clears_transactionally() {
        let mut state = State::new(
            ModGrants {
                hud: true,
                ..Default::default()
            },
            String::new(),
            Default::default(),
        );
        let replacement = r#"{"cards":[],"hide_effect_icons":true}"#;
        assert!(set_content(&mut state, replacement.into()).unwrap().is_ok());
        assert!(
            state.hud.content.is_none(),
            "publication waits for a successful callback"
        );
        state.hud.commit();
        assert!(state.hud.content.as_ref().unwrap().hide_effect_icons);
        assert!(
            set_content(
                &mut state,
                r#"{"cards":[],"hide_effect_icons":"true"}"#.into()
            )
            .unwrap()
            .is_err()
        );
        state.hud.commit();
        assert!(
            state.hud.content.as_ref().unwrap().hide_effect_icons,
            "invalid writes retain the committed HUD"
        );
        assert!(
            set_content(
                &mut state,
                r#"{"cards":[],"hide_effect_icons":false}"#.into()
            )
            .unwrap()
            .is_ok()
        );
        state.hud.commit();
        assert!(state.hud.content.is_none());
        assert!(set_content(&mut state, replacement.into()).unwrap().is_ok());
        state.hud.commit();
        assert!(set_content(&mut state, String::new()).unwrap().is_ok());
        state.hud.commit();
        assert!(
            state.hud.content.is_none(),
            "clearing restores normal effect icons"
        );
    }

    #[test]
    fn editor_requests_require_both_grants_focused_panel_and_successful_commit() {
        let preview = r#"{"cards":[{"id":"equipment","rows":[]}]}"#;
        for grants in [
            ModGrants::default(),
            ModGrants {
                hud: true,
                ..Default::default()
            },
            ModGrants {
                controls: true,
                ..Default::default()
            },
        ] {
            let mut state = State::new(grants, String::new(), Default::default());
            state.controls.frame.focused = true;
            state.controls.frame.panel_open = true;
            assert!(open_editor(&mut state, preview.into()).unwrap().is_err());
            state.hud.commit();
            assert!(state.hud.editor_request.is_none());
        }
        let mut state = State::new(
            ModGrants {
                hud: true,
                controls: true,
                ..Default::default()
            },
            String::new(),
            Default::default(),
        );
        assert!(open_editor(&mut state, preview.into()).unwrap().is_err());
        state.controls.frame.focused = true;
        state.controls.frame.panel_open = true;
        assert!(open_editor(&mut state, preview.into()).unwrap().is_ok());
        assert!(
            state.hud.editor_request.is_none(),
            "callback output remains staged"
        );
        state.hud.commit();
        assert_eq!(
            state.hud.editor_request.as_ref().unwrap().cards[0].id,
            "equipment"
        );
        assert!(
            open_editor(&mut state, r#"{"cards":[]}"#.into())
                .unwrap()
                .is_ok()
        );
        state.hud.commit();
        assert!(state.hud.editor_request.as_ref().unwrap().cards.is_empty());
    }
    #[test]
    fn editor_result_is_stable_during_callback_and_consumed_after_commit() {
        let mut state = State::new(
            ModGrants {
                hud: true,
                controls: true,
                ..Default::default()
            },
            String::new(),
            Default::default(),
        );
        state.hud.editor_result = Some(EditorResult {
            saved: true,
            reset: true,
            placements: vec![ui::mod_hud::Placement {
                id: "equipment".into(),
                scale: 1.5,
                position: Some([0.25, 0.75]),
            }],
        });
        let layout = read_editor_layout(&mut state).unwrap().unwrap().unwrap();
        assert_eq!(layout.placements[0].scale, 1.5);
        assert_eq!(layout.placements[0].position.as_ref().unwrap().x, 0.25);
        for _ in 0..2 {
            let result = read_editor_result(&mut state).unwrap().unwrap().unwrap();
            assert!(result.saved && result.reset);
            assert_eq!(result.placements.len(), 1);
            assert_eq!(result.placements[0].position.as_ref().unwrap().x, 0.25);
        }
        assert!(state.hud.editor_result.is_some());
        state.hud.commit();
        assert!(state.hud.editor_result.is_none());
        assert!(read_editor_result(&mut state).unwrap().unwrap().is_none());
    }
    #[test]
    fn bounded_json_and_unknown_fields_fail_without_staging_output() {
        let mut state = State::new(
            ModGrants {
                hud: true,
                ..Default::default()
            },
            String::new(),
            Default::default(),
        );
        assert!(
            set_content(&mut state, "x".repeat(MAX_HUD_BYTES + 1))
                .unwrap()
                .is_err()
        );
        assert!(
            set_content(&mut state, r#"{"cards":[],"texture":"external"}"#.into())
                .unwrap()
                .is_err()
        );
        assert!(
            set_crosshair(&mut state, "x".repeat(MAX_CROSSHAIR_BYTES + 1))
                .unwrap()
                .is_err()
        );
        assert!(
            set_crosshair(&mut state, r#"{"size":500}"#.into())
                .unwrap()
                .is_err()
        );
        state.hud.commit();
        assert!(state.hud.content.is_none());
        assert!(state.hud.crosshair.is_none());
    }
}

/// Stages local system feedback without creating an outbound chat or command packet.
pub(super) fn local_message(state: &mut State, text: String) -> Result<Result<(), String>> {
    if let Err(error) = admit(state)? {
        return Ok(Err(error));
    }
    if text.is_empty()
        || text.len() > super::MAX_LABEL_BYTES
        || text.chars().any(|c| c.is_control() || c == '§')
    {
        return Ok(Err("local message must be short plain text".into()));
    }
    if state.hud.pending_messages.len() + state.hud.local_messages.len() >= 4 {
        return Ok(Err("local message queue is full".into()));
    }
    state.hud.pending_messages.push(text);
    Ok(Ok(()))
}

#[cfg(test)]
mod local_message_tests {
    use super::*;
    /// Native cosmetic policies survive even when all draggable cards are hidden.
    #[test]
    fn cosmetic_only_content_is_retained_and_cleared_explicitly() {
        let mut state = State::new(
            super::super::ModGrants {
                hud: true,
                ..Default::default()
            },
            String::new(),
            Default::default(),
        );
        for json in [
            r#"{"cards":[],"actor_style":{"hurt_color":[1,0,0,1]}}"#,
            r#"{"cards":[],"hunger_overlay":{"saturation":true}}"#,
        ] {
            assert!(set_content(&mut state, json.into()).unwrap().is_ok());
            state.hud.commit();
            assert!(state.hud.content.is_some());
        }
        assert!(set_content(&mut state, String::new()).unwrap().is_ok());
        state.hud.commit();
        assert!(state.hud.content.is_none());
    }
    /// Messages are unavailable until commit and remain bounded without consumer progress.
    #[test]
    fn feedback_is_transactional_bounded_and_requires_hud_authority() {
        let mut state = State::new(
            super::super::ModGrants::default(),
            String::new(),
            Default::default(),
        );
        assert!(local_message(&mut state, "Notice".into()).unwrap().is_err());
        state.grants.hud = true;
        assert!(
            local_message(&mut state, "bad\nline".into())
                .unwrap()
                .is_err()
        );
        for _ in 0..4 {
            assert!(local_message(&mut state, "Notice".into()).unwrap().is_ok());
        }
        assert!(state.hud.local_messages.is_empty());
        assert!(local_message(&mut state, "Excess".into()).unwrap().is_err());
        state.hud.commit();
        assert_eq!(state.hud.local_messages, vec!["Notice"; 4]);
        assert!(local_message(&mut state, "Excess".into()).unwrap().is_err());
    }
}
