use super::*;

#[test]
fn custom_mining_completes_at_component_time_under_both_authorities() {
    for authority in [Client, Server] {
        for (seconds, ticks) in [(2.0_f32, 60), (0.5, 15), (-1.0, 120)] {
            let mut target = target([0, 0, 0], "test:custom", None);
            target.block = sim::BlockDestroyInfo::from_facts(
                assets::compiled::block_destroy::BlockDestroyFacts::Custom {
                    seconds_to_destroy_bits: seconds.to_bits(),
                    effective_tools: 0,
                },
            );
            let mut machine = DestroyMachine::default();
            assert!(
                !held(&mut machine, &target, authority)
                    .actions
                    .iter()
                    .any(|action| action.kind == PredictDestroy)
            );
            for tick in 1..=ticks {
                let payload = held(&mut machine, &target, authority);
                let completed = match authority {
                    Server => payload
                        .actions
                        .iter()
                        .any(|action| action.kind == PredictDestroy),
                    Client => payload.destroy.is_some(),
                };
                assert_eq!(
                    completed,
                    seconds >= 0.0 && tick == ticks,
                    "{authority:?} {seconds} tick {tick}"
                );
            }
        }
    }
}
