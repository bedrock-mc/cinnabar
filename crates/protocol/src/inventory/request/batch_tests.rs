use super::*;
use bytes::BytesMut;
use valentine::bedrock::{codec::BedrockCodec, version::v1_26_51::McpePacketData};

/// Produces a created-output consumption with a current or earlier request ID.
fn consume(id: i32) -> [StackRequestAction; 1] {
    [StackRequestAction::Consume {
        amount: 1,
        source: StackRequestSlot {
            container: StackRequestContainer::CreatedOutput,
            slot: CREATED_OUTPUT_SLOT,
            stack_network_id: id,
        },
    }]
}

#[test]
fn batched_requests_keep_ids_actions_and_filter_origins_separate_on_wire() {
    let first = consume(-3);
    let second = consume(-5);
    let strings = [String::from("renamed")];
    let packet = item_stack_request_batch([
        (-3, first.as_slice(), strings.as_slice()),
        (-5, second.as_slice(), &[]),
    ])
    .unwrap()
    .unwrap();
    let McpePacketData::ItemStackRequestPacket(request) = packet.data else {
        panic!("request packet");
    };
    let mut bytes = BytesMut::new();
    request.encode(&mut bytes).unwrap();
    let decoded = ItemStackRequestPacket::decode(&mut bytes.freeze(), ()).unwrap();
    assert_eq!(decoded.requests.len(), 2);
    assert_eq!(
        decoded
            .requests
            .iter()
            .map(|r| r.client_request_id.id)
            .collect::<Vec<_>>(),
        [-3, -5]
    );
    assert!(decoded.requests.iter().all(|r| r.actions.len() == 1));
    assert_eq!(decoded.requests[0].strings_to_filter, strings);
    assert_eq!(
        decoded.requests[0].strings_to_filter_origin,
        EnumsTextProcessingEventOrigin::Anviltext
    );
    assert!(decoded.requests[1].strings_to_filter.is_empty());
    assert_eq!(
        decoded.requests[1].strings_to_filter_origin,
        EnumsTextProcessingEventOrigin::Unknown
    );
}

#[test]
fn batch_preserves_prior_sparse_ids_without_admitting_future_ids() {
    let first = consume(-3);
    let packet = item_stack_request_batch([
        (-3, first.as_slice(), &[][..]),
        (-5, first.as_slice(), &[][..]),
    ])
    .unwrap()
    .unwrap();
    let McpePacketData::ItemStackRequestPacket(request) = packet.data else {
        panic!("request packet");
    };
    let mut bytes = BytesMut::new();
    request.encode(&mut bytes).unwrap();
    let decoded = ItemStackRequestPacket::decode(&mut bytes.freeze(), ()).unwrap();
    assert_eq!(decoded, request);
    assert_eq!(decoded.requests[0].client_request_id.id, -3);
    assert_eq!(decoded.requests[1].client_request_id.id, -5);
    let future = consume(-5);
    assert!(
        item_stack_request_batch([
            (-3, future.as_slice(), &[][..]),
            (-5, future.as_slice(), &[][..])
        ])
        .is_err()
    );
}

#[test]
fn empty_batch_does_not_emit_a_packet() {
    assert!(item_stack_request_batch([]).unwrap().is_none());
}
