use super::*;
use assets::{MOLANG_QUERY_DESCRIPTORS, MolangQuerySupport};

#[test]
fn every_admitted_query_has_a_dispatch_or_an_explicit_idle_disposition() {
    let actor = crate::actor_animation::tests::actor_with_metadata(HashMap::new());
    let input = ActorTickInput::default();
    let context = ActorTickContext::default();
    let inputs = QueryInputs {
        actor: &actor,
        input: &input,
        context: &context,
        anim_tick: 0,
        anim_time: None,
        swell_amount: None,
        life_tick: 0,
        finished: (false, false),
        state_time: 0.0,
        bones: &[],
        bone_names: &[],
    };
    for descriptor in MOLANG_QUERY_DESCRIPTORS {
        let binding = Q::from_name(descriptor.name).unwrap();
        for arguments in [
            vec![],
            vec![MolangValue::Number(0.0)],
            vec![
                MolangValue::String("fixture:property".into()),
                MolangValue::Number(0.0),
            ],
        ] {
            let value = query(&inputs, binding, &arguments);
            if descriptor.support == MolangQuerySupport::Unimplemented {
                assert_eq!(value.number(), 0.0, "{}", descriptor.name);
            }
        }
    }
}
