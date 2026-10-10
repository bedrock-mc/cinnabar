use super::*;
use crate::ModGrants;
use wit::Host as _;

fn state() -> State {
    let grants = ModGrants {
        render: true,
        ..Default::default()
    };
    State::new(grants, String::new(), Default::default())
}

#[test]
fn unchanged_pass_updates_stage_nothing() {
    let mut state = state();
    let source = "fn effect(uv: vec2<f32>) -> vec3<f32> { return scene(uv); }";
    let spec = wit::PassSpec {
        name: "grade".into(),
        order: 0,
        source: source.into(),
        depth: false,
    };
    state.register_pass(spec).unwrap().unwrap();
    state
        .update_pass("grade".into(), true, vec![0.5])
        .unwrap()
        .unwrap();
    state.render.commit();
    state.render.begin_frame();
    state
        .update_pass("grade".into(), true, vec![0.5])
        .unwrap()
        .unwrap();
    assert!(
        state.render.staged_passes.is_none(),
        "a no-op update copies no passes"
    );
    state
        .update_pass("grade".into(), true, vec![0.75])
        .unwrap()
        .unwrap();
    assert!(state.render.staged_passes.is_some());
    assert!(
        state
            .update_pass("missing".into(), true, vec![])
            .unwrap()
            .is_err()
    );
}
