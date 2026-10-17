//! Local wall-clock capture stays at the host boundary; formatting accepts explicit observations.

use chrono::Timelike;
use ui::mod_hud::{Hud, NativeRows};

/// One local minute, independent of Minecraft time and elapsed callback time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct LocalMinute {
    hour: u8,
    minute: u8,
}

/// Reads the OS clock only for an active native clock policy.
pub(super) fn observe(hud: &Hud) -> Option<LocalMinute> {
    if !hud.cards.iter().any(|c| {
        c.source
            .as_ref()
            .is_some_and(|s| matches!(s.kind, NativeRows::Clock { .. }))
    }) {
        return None;
    }
    let now = chrono::Local::now();
    Some(LocalMinute {
        hour: now.hour() as u8,
        minute: now.minute() as u8,
    })
}

/// Formats an explicit local minute without advancing a timer or guessing the system timezone.
fn format_time(now: LocalMinute, military: bool, am_pm: bool) -> String {
    if military {
        format!("{:02}:{:02}", now.hour, now.minute)
    } else {
        let hour = if now.hour.is_multiple_of(12) {
            12
        } else {
            now.hour % 12
        };
        let suffix = if !am_pm {
            ""
        } else if now.hour < 12 {
            " AM"
        } else {
            " PM"
        };
        format!("{hour}:{:02}{suffix}", now.minute)
    }
}

/// Resolves clock rows before generic sources remove their admitted policy.
pub(super) fn hydrate(hud: &mut Hud, now: Option<LocalMinute>) {
    for card in &mut hud.cards {
        let Some(source) = &card.source else { continue };
        let NativeRows::Clock { military, am_pm } = source.kind else {
            continue;
        };
        card.rows = vec![super::sources::row(
            source,
            "",
            now.map_or_else(|| "--".into(), |now| format_time(now, military, am_pm)),
        )];
        card.source = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Midnight, noon, afternoon and disabled suffixes are deterministic at every minute.
    #[test]
    fn clock_formats_explicit_local_minutes() {
        for (hour, minute, military, am_pm, want) in [
            (0, 5, false, true, "12:05 AM"),
            (12, 0, false, true, "12:00 PM"),
            (23, 59, false, true, "11:59 PM"),
            (13, 7, false, false, "1:07"),
            (0, 5, true, true, "00:05"),
            (23, 59, true, false, "23:59"),
        ] {
            assert_eq!(
                format_time(LocalMinute { hour, minute }, military, am_pm),
                want
            );
        }
    }
    /// Native minutes update independently of guest callbacks and preserve the cached row while idle.
    #[test]
    fn native_clock_updates_without_guest_polling_and_clears_on_revocation() {
        use super::super::State;
        use crate::ModGrants;
        let mut state = State::new(
            ModGrants {
                hud: true,
                ..Default::default()
            },
            String::new(),
            Default::default(),
        );
        let content:Hud=serde_json::from_str(r#"{"cards":[{"id":"clock","rows":[],"source":{"kind":{"kind":"clock","military":false,"am_pm":true},"limit":1},"text_plate":{}}]}"#).unwrap();
        content.validate().unwrap();
        super::super::sources::admit(&state, &content).unwrap();
        state.hud.content = Some(content);
        let minute = LocalMinute {
            hour: 23,
            minute: 59,
        };
        super::super::sources::refresh_with_clock(&mut state, Some(minute));
        assert_eq!(
            state.hud.resolved.as_ref().unwrap().cards[0].rows[0].value,
            "11:59 PM"
        );
        let pointer = state.hud.resolved.as_ref().unwrap().cards[0].rows[0]
            .value
            .as_ptr();
        state.player_state.world = Some(crate::WorldObservation {
            session: 1,
            world: "clock-independent-world".into(),
            dimension: 0,
            position: (10., 64., 10.),
            alive: true,
        });
        super::super::sources::refresh_with_clock(&mut state, Some(minute));
        assert_eq!(
            pointer,
            state.hud.resolved.as_ref().unwrap().cards[0].rows[0]
                .value
                .as_ptr()
        );
        super::super::sources::refresh_with_clock(
            &mut state,
            Some(LocalMinute { hour: 0, minute: 0 }),
        );
        assert_eq!(
            state.hud.resolved.as_ref().unwrap().cards[0].rows[0].value,
            "12:00 AM"
        );
        state.hud.content = None;
        super::super::sources::refresh_with_clock(&mut state, None);
        assert!(state.hud.resolved.is_none());
    }
}
