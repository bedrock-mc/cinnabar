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

/// Additive line drawing preserves legacy ribbons and rejects overflow atomically.
#[test]
fn solid_lines_share_the_legacy_ribbon_budget() {
    let mut state = state();
    let ribbon = wit::Ribbon {
        points: vec![
            wit::Vector3 {
                x: 0.,
                y: 0.,
                z: 0.,
            },
            wit::Vector3 {
                x: 1.,
                y: 1.,
                z: 1.,
            },
        ],
        width: 0.01,
        color: wit::Rgba {
            r: 1.,
            g: 1.,
            b: 1.,
            a: 1.,
        },
    };
    state
        .draw(wit::Primitives {
            decals: vec![],
            ribbons: vec![ribbon.clone(); mod_api::MAX_RENDER_RIBBONS - 1],
            beams: vec![],
            billboards: vec![],
        })
        .unwrap()
        .unwrap();
    let line = wit::Line {
        points: ribbon.points.clone(),
        width: 1.,
        color: ribbon.color,
        pattern: wit::LinePattern::Solid,
    };
    state.draw_lines(vec![line.clone()]).unwrap().unwrap();
    assert!(state.render.staged_primitives.ribbons[0].taper);
    assert!(!state.render.staged_primitives.ribbons.last().unwrap().taper);
    assert!(state.draw_lines(vec![line]).unwrap().is_err());
    assert_eq!(
        state.render.staged_primitives.ribbons.len(),
        mod_api::MAX_RENDER_RIBBONS
    );
}
