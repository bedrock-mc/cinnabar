use super::*;

/// Repeated frames perform no upload work; changed slots/counts and invalid commands retry.
#[test]
fn indirect_commands_are_built_once_per_changed_valid_identity() {
    let mut last = None;
    let mut builds = 0;
    for identity in [(0, 10), (0, 10), (1, 10), (1, 11), (1, 11)] {
        let command = indirect_upload(&mut last, identity, || {
            builds += 1;
            Some(DrawIndexedIndirectArgs {
                index_count: 6,
                instance_count: identity.1 as u32,
                first_index: 0,
                base_vertex: 0,
                first_instance: 0,
            })
        });
        if let Some(command) = command {
            assert_eq!(command.instance_count, identity.1 as u32);
        }
    }
    assert_eq!(builds, 3);
    assert!(indirect_upload(&mut last, (0, 20), || None).is_none());
    assert_eq!(
        last,
        Some((1, 11)),
        "an invalid command cannot poison admission"
    );
    assert!(
        indirect_upload(&mut last, (0, 20), || Some(DrawIndexedIndirectArgs {
            index_count: 6,
            instance_count: 20,
            first_index: 0,
            base_vertex: 0,
            first_instance: 0
        }))
        .is_some()
    );
}
