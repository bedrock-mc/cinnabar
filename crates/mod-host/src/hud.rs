//! Transactional cosmetic presentation, with no input or world authority.

use super::{MAX_IMPORT_WRITES, State};
use anyhow::{Result, bail};
use ui::mod_hud::{Crosshair, EditorResult, Hud, MAX_CROSSHAIR_BYTES, MAX_HUD_BYTES};

#[derive(Default)]
pub(super) struct HudState {
    pub content: Option<Hud>,
    pub crosshair: Option<Crosshair>,
    pending_content: Option<Option<Hud>>,
    pending_crosshair: Option<Option<Crosshair>>,
    pub editor_request: Option<Hud>,
    pub editor_result: Option<EditorResult>,
    pending_editor: Option<Hud>,
    result_read: bool,
}
impl HudState {
    pub fn commit(&mut self) {
        if self.result_read {
            self.editor_result = None;
            self.result_read = false;
        }
        if let Some(content) = self.pending_content.take() {
            self.content = content;
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
    let preview: Hud = match serde_json::from_str(&json) {
        Ok(preview) => preview,
        Err(error) => return Ok(Err(format!("invalid HUD preview: {error}"))),
    };
    if let Err(error) = preview.validate() {
        return Ok(Err(error));
    }
    if preview.cards.is_empty() {
        return Ok(Err("HUD editor requires preview cards".into()));
    }
    state.hud.pending_editor = Some(preview);
    Ok(Ok(()))
}

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
        (!hud.cards.is_empty()).then_some(hud)
    };
    state.hud.pending_content = Some(content);
    Ok(Ok(()))
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
            let mut state = State::new(grants, String::new());
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
        );
        state.hud.editor_result = Some(EditorResult {
            saved: true,
            reset: true,
            placements: vec![ui::mod_hud::Placement {
                id: "equipment".into(),
                position: Some([0.25, 0.75]),
            }],
        });
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
