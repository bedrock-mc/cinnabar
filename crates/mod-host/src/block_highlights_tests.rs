use super::*;
use crate::ModGrants;
use wit::Host as _;

fn state() -> State {
    State::new(
        ModGrants {
            block_highlights: true,
            ..Default::default()
        },
        String::new(),
        Default::default(),
    )
}
fn spec() -> wit::BlockHighlightSpec {
    wit::BlockHighlightSpec {
        identifiers: vec!["minecraft:ancient_debris".into()],
        range: 64.0,
        color: wit::Rgba {
            r: 1.0,
            g: 0.1,
            b: 0.7,
            a: 0.8,
        },
    }
}
#[test]
fn block_highlights_require_their_own_grant() {
    let mut state = State::new(
        ModGrants {
            render: true,
            ..Default::default()
        },
        String::new(),
        Default::default(),
    );
    assert!(state.set_block_highlights(Some(spec())).unwrap().is_err());
    assert!(state.set_block_highlights(None).unwrap().is_err());
    state.block_highlights.commit();
    assert!(state.block_highlights.committed.is_none());
}
#[test]
fn block_highlights_validate_names_range_and_colour_before_staging() {
    let mut state = state();
    let mut cases = Vec::new();
    for names in [
        vec![],
        vec!["minecraft:ancient_debris".into(); MAX_BLOCK_HIGHLIGHT_IDENTIFIERS + 1],
        vec!["debris".into()],
        vec!["minecraft:".into()],
        vec!["minecraft:debris\n".into()],
        vec![format!(
            "minecraft:{}",
            "a".repeat(MAX_BLOCK_HIGHLIGHT_IDENTIFIER_BYTES)
        )],
    ] {
        let mut value = spec();
        value.identifiers = names;
        cases.push(value);
    }
    for range in [
        0.0,
        MAX_BLOCK_HIGHLIGHT_RANGE + 1.0,
        f32::NAN,
        f32::INFINITY,
    ] {
        let mut value = spec();
        value.range = range;
        cases.push(value);
    }
    for channel in [-0.1, 1.1, f32::NAN, f32::INFINITY] {
        let mut value = spec();
        value.color.a = channel;
        cases.push(value);
    }
    for value in cases {
        state.block_highlights.begin_frame();
        assert!(state.set_block_highlights(Some(value)).unwrap().is_err());
        assert!(state.block_highlights.pending.is_none());
    }
    for range in [1.0, MAX_BLOCK_HIGHLIGHT_RANGE] {
        let mut value = spec();
        value.range = range;
        state.set_block_highlights(Some(value)).unwrap().unwrap();
    }
}
#[test]
fn block_highlights_commit_retain_clear_and_revoke_transactionally() {
    let mut state = state();
    state.set_block_highlights(Some(spec())).unwrap().unwrap();
    assert!(state.block_highlights.committed.is_none());
    state.block_highlights.commit();
    let expected = state.block_highlights.committed.clone();
    state.block_highlights.begin_frame();
    state.block_highlights.commit();
    assert_eq!(state.block_highlights.committed, expected);
    let pointer = state
        .block_highlights
        .committed
        .as_ref()
        .unwrap()
        .identifiers
        .as_ptr();
    state.set_block_highlights(Some(spec())).unwrap().unwrap();
    state.block_highlights.commit();
    assert_eq!(
        state
            .block_highlights
            .committed
            .as_ref()
            .unwrap()
            .identifiers
            .as_ptr(),
        pointer
    );
    state.set_block_highlights(None).unwrap().unwrap();
    assert_eq!(state.block_highlights.committed, expected);
    state.block_highlights.begin_frame();
    state.block_highlights.commit();
    assert_eq!(
        state.block_highlights.committed, expected,
        "an abandoned callback never commits"
    );
    state.set_block_highlights(None).unwrap().unwrap();
    state.block_highlights.commit();
    assert!(state.block_highlights.committed.is_none());
    state.set_block_highlights(Some(spec())).unwrap().unwrap();
    state.block_highlights.commit();
    state.set_block_highlights(Some(spec())).unwrap().unwrap();
    state.block_highlights.revoke();
    assert!(state.block_highlights.committed.is_none());
    assert!(state.block_highlights.pending.is_none());
    assert!(
        HighlightState::default().committed.is_none(),
        "replacement instances inherit no selection"
    );
}
#[test]
fn block_highlight_set_and_clear_share_one_callback_budget() {
    let mut state = state();
    for index in 0..MAX_IMPORT_WRITES {
        state
            .set_block_highlights((index % 2 == 0).then(spec))
            .unwrap()
            .unwrap();
    }
    assert!(state.set_block_highlights(None).is_err());
    state.block_highlights.begin_frame();
    state.set_block_highlights(None).unwrap().unwrap();
}
