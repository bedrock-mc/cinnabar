use super::*;

/// Recovery sends need no input grant, and the strike that stops a part drops them.
#[test]
fn recovery_messages_publish_without_input_and_stop_at_the_strike_limit() {
    let mut live = failing();
    let instance = live.instances.get_mut("bundle0").unwrap();
    instance.capabilities.scope.permissions = BTreeSet::from([Permission::Messaging]);
    instance.capabilities.actions.clear();
    instance.events.clear();
    instance.shift = None;

    for strike in 1..=MAX_GUEST_STRIKES {
        let now = strike as u64 * CALLBACK_INTERVAL_MS;
        receive(&mut live, "bundle0", strike as u64);
        live.poll(1, now).unwrap();
        complete(&mut live, "bundle0");
        let packets = live.poll(1, now).unwrap();
        let instance = &live.instances["bundle0"];
        assert_eq!(instance.strikes.len(), strike);
        if strike == MAX_GUEST_STRIKES {
            assert!(packets.is_empty());
            assert!(instance.helper.is_none());
        } else {
            assert_eq!(packets.len(), 1);
            let send: Envelope = serde_json::from_slice(&packets[0]).unwrap();
            assert_eq!(send.channel, "bundle0.events1");
            assert_eq!(send.payload, vec![Scalar::Bool(true)]);
            assert_eq!(send.world_epoch, 1);
            assert!(!instance.busy);
            assert!(instance.events.is_empty());
            let dispatched = &instance.helper.as_ref().unwrap().dispatched;
            assert_eq!(dispatched.len(), strike);
            assert!(
                dispatched
                    .iter()
                    .all(|event| matches!(event, Event::Message { .. }))
            );
        }
    }
}
