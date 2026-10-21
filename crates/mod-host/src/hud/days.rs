//! Server-day binding keeps clock facts native and avoids guest polling on every frame.

use super::super::State;
use crate::module_observations::WorldDay;
use ui::mod_hud::{Hud, NativeRows};

/// Rejects missing grants and clocks outside the currently presented session and dimension.
pub(super) fn observe(state: &State) -> Option<WorldDay> {
    if !state.grants.player_state {
        return None;
    }
    let day = state.player_state.observations.world_day?;
    let world = state.player_state.world.as_ref()?;
    (day.session != 0 && day.session == world.session && day.dimension == world.dimension)
        .then_some(day)
}

/// Resolves a single bounded value before generic inventory sources consume their policies.
pub(super) fn hydrate(hud: &mut Hud, day: Option<WorldDay>) {
    for card in &mut hud.cards {
        let Some(source) = &card.source else {
            continue;
        };
        if !matches!(source.kind, NativeRows::WorldDays) {
            continue;
        }
        card.rows = vec![super::sources::row(
            source,
            "",
            if card.sample {
                "12 Days".into()
            } else {
                day.map_or_else(|| "-- Days".into(), |day| format!("{} Days", day.day))
            },
        )];
        card.source = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ModGrants;

    /// Builds a real host policy and admits it only after its independent player-state grant.
    fn state() -> State {
        let mut state = State::new(
            ModGrants {
                hud: true,
                ..Default::default()
            },
            String::new(),
            Default::default(),
        );
        let json = r#"{"cards":[{"id":"days","rows":[],"source":{"kind":{"kind":"world_days"},"limit":1},"text_plate":{}}]}"#;
        assert!(
            super::super::set_content(&mut state, json.into())
                .unwrap()
                .is_err()
        );
        state.grants.player_state = true;
        super::super::set_content(&mut state, json.into())
            .unwrap()
            .unwrap();
        state.hud.commit();
        state.player_state.world = Some(crate::WorldObservation {
            session: 7,
            dimension: 0,
            world: "fixture".into(),
            position: (0., 64., 0.),
            alive: true,
        });
        state
    }

    /// Day zero is real; missing grants, world scope or clock data always remain unknown.
    #[test]
    fn world_days_follow_native_scope_and_never_reuse_stale_session_or_dimension_data() {
        let mut state = state();
        for (day, expected) in [
            (None, "-- Days"),
            (
                Some(WorldDay {
                    session: 7,
                    dimension: 0,
                    day: 0,
                }),
                "0 Days",
            ),
            (
                Some(WorldDay {
                    session: 7,
                    dimension: 0,
                    day: 12,
                }),
                "12 Days",
            ),
            (
                Some(WorldDay {
                    session: 8,
                    dimension: 0,
                    day: 99,
                }),
                "-- Days",
            ),
            (
                Some(WorldDay {
                    session: 7,
                    dimension: 1,
                    day: 99,
                }),
                "-- Days",
            ),
        ] {
            state.player_state.observations.world_day = day;
            super::super::sources::refresh(&mut state);
            let hud = state.hud.resolved.as_ref().unwrap();
            assert_eq!(hud.cards[0].rows[0].value, expected);
            hud.validate().unwrap();
        }
        state.player_state.observations.world_day = Some(WorldDay {
            session: 7,
            dimension: 0,
            day: 12,
        });
        state.grants.player_state = false;
        assert!(observe(&state).is_none());
        state.grants.player_state = true;
        state.player_state.world = None;
        assert!(observe(&state).is_none());
    }

    /// Walking does not rebuild a day-only HUD from changing positions or unrelated inventory facts.
    #[test]
    fn unchanged_world_day_reuses_the_resolved_rows_while_the_player_moves() {
        let mut state = state();
        state.player_state.observations.world_day = Some(WorldDay {
            session: 7,
            dimension: 0,
            day: 12,
        });
        super::super::sources::refresh(&mut state);
        let rows = state.hud.resolved.as_ref().unwrap().cards[0].rows.as_ptr();
        state.player_state.world.as_mut().unwrap().position.0 += 10.;
        super::super::sources::refresh(&mut state);
        assert_eq!(
            rows,
            state.hud.resolved.as_ref().unwrap().cards[0].rows.as_ptr()
        );
        let mut preview = state.hud.content.clone().unwrap();
        hydrate(&mut preview, observe(&state));
        super::super::sources::hydrate_preview(&mut preview, None, None);
        assert_eq!(preview.cards[0].rows[0].value, "12 Days");
        preview.validate().unwrap();
    }
}
