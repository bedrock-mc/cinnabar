//! Additive visibility output shares the existing editor callback transaction.

use super::*;

/// Returns hidden IDs until a successful callback consumes any editor result read.
pub(crate) fn read_editor_visibility(
    state: &mut State,
) -> Result<Result<Option<crate::runtime::cinnabar::extension::hud::VisibilityResult>, String>> {
    if let Err(error) = admit(state)? {
        return Ok(Err(error));
    }
    if !state.grants.controls {
        return Ok(Err("controls capability denied".into()));
    }
    state.hud.result_read = true;
    Ok(Ok(state.hud.editor_result.as_ref().map(|result| {
        crate::runtime::cinnabar::extension::hud::VisibilityResult {
            saved: result.saved,
            hidden_cards: result.hidden_cards.clone(),
        }
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ModGrants, runtime::cinnabar::extension::hud::Host};

    #[test]
    fn studio_visibility_read_is_stable_authorized_and_consumed_with_layout() {
        for (hud, controls) in [(true, true), (true, false), (false, true)] {
            let mut state = State::new(
                ModGrants {
                    hud,
                    controls,
                    ..Default::default()
                },
                String::new(),
                Default::default(),
            );
            state.hud.editor_result = Some(EditorResult {
                saved: true,
                hidden_cards: vec!["fps".into()],
                ..Default::default()
            });
            if !hud || !controls {
                assert!(Host::read_editor_visibility(&mut state).unwrap().is_err());
                assert!(!state.hud.result_read);
                continue;
            }
            for _ in 0..2 {
                let result = Host::read_editor_visibility(&mut state)
                    .unwrap()
                    .unwrap()
                    .unwrap();
                assert!(result.saved);
                assert_eq!(result.hidden_cards, ["fps"]);
                assert!(
                    Host::read_editor_layout(&mut state)
                        .unwrap()
                        .unwrap()
                        .is_some()
                );
            }
            state.hud.commit();
            assert!(
                Host::read_editor_visibility(&mut state)
                    .unwrap()
                    .unwrap()
                    .is_none()
            );
        }
    }
}

#[cfg(test)]
mod reset_tests {
    use super::*;
    use crate::{ModGrants, runtime::cinnabar::extension::hud::Host};

    #[test]
    fn studio_history_restore_replaces_unread_reset_through_guest_delivery() {
        let mut state = State::new(
            ModGrants {
                hud: true,
                controls: true,
                ..Default::default()
            },
            String::new(),
            Default::default(),
        );
        state.hud.deliver_editor_result(EditorResult {
            saved: true,
            reset: true,
            ..Default::default()
        });
        state.hud.deliver_editor_result(EditorResult {
            saved: true,
            ..Default::default()
        });
        assert!(state.hud.editor_result.as_ref().unwrap().reset);
        state.hud.deliver_editor_result(EditorResult {
            saved: true,
            replace_pending_reset: true,
            ..Default::default()
        });
        state.hud.deliver_editor_result(EditorResult {
            saved: true,
            ..Default::default()
        });
        let result = Host::read_editor_layout(&mut state)
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(result.saved && !result.reset);
        state.hud.commit();
        assert!(state.hud.editor_result.is_none());
    }
}
