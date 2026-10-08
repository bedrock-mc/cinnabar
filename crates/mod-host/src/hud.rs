//! Transactional cosmetic presentation, with no input or world authority.

use super::{MAX_IMPORT_WRITES, State};
use anyhow::{Result, bail};
use ui::mod_hud::{Crosshair, Hud, MAX_CROSSHAIR_BYTES, MAX_HUD_BYTES};

#[derive(Default)]
pub(super) struct HudState {
    pub content: Option<Hud>,
    pub crosshair: Option<Crosshair>,
    pending_content: Option<Option<Hud>>,
    pending_crosshair: Option<Option<Crosshair>>,
}
impl HudState {
    pub fn commit(&mut self) {
        if let Some(content) = self.pending_content.take() {
            self.content = content;
        }
        if let Some(crosshair) = self.pending_crosshair.take() {
            self.crosshair = crosshair;
        }
    }
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
