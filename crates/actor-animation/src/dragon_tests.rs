use super::*;

/// The flap query and historical engine variables read the actor's dragon state by offset and axis.
#[test]
fn dragon_query_and_engine_history_read_the_actor_dragon_state() {
    let mut actor = TestActor::default();
    actor.status.dragon_flap_phase = Some(0.2);
    actor.status.dragon_history = vec![[0.0; 2]; 24];
    actor.status.dragon_history[0] = [-179.0, 64.0];
    actor.status.dragon_history[23] = [10.0, 70.0];
    let input = ActorTickInput::default();
    let context = ActorTickContext::default();
    let inputs = query::QueryInputs {
        actor: &actor,
        input: &input,
        context: &context,
        anim_tick: 1,
        anim_time: None,
        swell_amount: None,
        life_tick: 1,
        finished: (false, false),
        state_time: 0.0,
        bones: &[],
        bone_names: &[],
    };
    assert_eq!(
        query::named_query(&inputs, "query.wing_flap_position", &[]).number(),
        0.2
    );
    let engine = EngineSlots {
        dragon_history: vec![(0, 0, 0), (1, 0, 1), (2, 23, 1)],
        ..Default::default()
    };
    let mut variables = MolangVariables::slots(3);
    tick::apply_engine_variables(
        &engine,
        &mut variables,
        &actor,
        &context,
        &input,
        &MotionState::default(),
    );
    assert_eq!(variables.number_at(0), Some(-179.0));
    assert_eq!(variables.number_at(1), Some(64.0));
    assert_eq!(variables.number_at(2), Some(70.0));
}
