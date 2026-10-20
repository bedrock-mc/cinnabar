//! The additive WIT read preserves anchors and the existing callback transaction.

use super::*;
use crate::{
    ModGrants,
    runtime::cinnabar::extension::hud::{Anchor as WitAnchor, Host},
};
use ui::mod_hud::{Anchor, Placement};

/// Creates the same saved placement for granted and denied guest callbacks.
fn state(grants: ModGrants, anchor: Anchor) -> State {
    let mut state = State::new(grants, String::new(), Default::default());
    state.hud.editor_result = Some(EditorResult {
        saved: true,
        reset: true,
        placements: vec![Placement {
            id: "fixture".into(),
            anchor,
            offset: [11., -7.],
            scale: 1.5,
            position: Some([0.25, 0.75]),
        }],
    });
    state
}

#[test]
fn anchored_wit_read_round_trips_all_anchors_and_shares_legacy_consumption() {
    for name in [
        "top_left",
        "top_center",
        "top_right",
        "left",
        "center",
        "right",
        "bottom_left",
        "bottom_center",
        "bottom_right",
    ] {
        let anchor: Anchor = serde_json::from_value(serde_json::json!(name)).unwrap();
        let mut state = state(
            ModGrants {
                hud: true,
                controls: true,
                ..Default::default()
            },
            anchor,
        );
        for _ in 0..2 {
            let result = Host::read_editor_anchored_layout(&mut state)
                .unwrap()
                .unwrap()
                .unwrap();
            assert!(result.saved && result.reset);
            assert_eq!(result.placements.len(), 1);
            let p = &result.placements[0];
            let name = match p.anchor {
                WitAnchor::TopLeft => "top_left",
                WitAnchor::TopCenter => "top_center",
                WitAnchor::TopRight => "top_right",
                WitAnchor::Left => "left",
                WitAnchor::Center => "center",
                WitAnchor::Right => "right",
                WitAnchor::BottomLeft => "bottom_left",
                WitAnchor::BottomCenter => "bottom_center",
                WitAnchor::BottomRight => "bottom_right",
            };
            let json = serde_json::json!({"cards":[{"id":p.id,"anchor":name,"offset":[p.offset.x,p.offset.y],"scale":p.scale,"rows":[]}]}).to_string();
            Host::set_content(&mut state, json).unwrap().unwrap();
            let normalized = Host::read_editor_layout(&mut state)
                .unwrap()
                .unwrap()
                .unwrap();
            assert_eq!(normalized.placements[0].position.as_ref().unwrap().x, 0.25);
            assert_eq!(normalized.placements[0].scale, 1.5);
            let legacy = Host::read_editor_result(&mut state)
                .unwrap()
                .unwrap()
                .unwrap();
            assert_eq!(legacy.placements[0].position.as_ref().unwrap().y, 0.75);
        }
        assert!(state.hud.editor_result.is_some());
        state.hud.commit();
        let card = &state.hud.content.as_ref().unwrap().cards[0];
        assert_eq!(card.anchor, anchor);
        assert_eq!(card.offset, [11., -7.]);
        assert_eq!(card.position, None);
        assert_eq!(card.scale, 1.5);
        state.writes = 0;
        assert!(
            Host::read_editor_anchored_layout(&mut state)
                .unwrap()
                .unwrap()
                .is_none()
        );
        assert!(
            Host::read_editor_layout(&mut state)
                .unwrap()
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn anchored_wit_read_requires_both_grants_and_cancellation_has_no_placements() {
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
        let mut state = state(grants, Anchor::Center);
        assert!(
            Host::read_editor_anchored_layout(&mut state)
                .unwrap()
                .is_err()
        );
        state.hud.commit();
        assert!(state.hud.editor_result.is_some());
    }
    let mut state = state(
        ModGrants {
            hud: true,
            controls: true,
            ..Default::default()
        },
        Anchor::Center,
    );
    state.hud.editor_result = Some(EditorResult::default());
    let result = Host::read_editor_anchored_layout(&mut state)
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(!result.saved && !result.reset && result.placements.is_empty());
}
