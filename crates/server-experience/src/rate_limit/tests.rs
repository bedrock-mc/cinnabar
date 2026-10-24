use super::*;
use crate::{
    negotiation::Wire,
    policy::*,
    session::Control,
    wire::{self, Envelope, Scalar},
};

#[test]
fn shared_rate_classes_and_encoded_costs_match() {
    let cases: serde_json::Value =
        serde_json::from_str(include_str!("../../schema/rate-conformance.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let class = case["control"].as_str().map_or(RateClass::Data, |raw| {
            let control: Control = serde_json::from_str(raw).unwrap();
            let direction = serde_json::from_value(case["direction"].clone()).unwrap();
            control.rate_class(direction, case["matching_notice"].as_bool().unwrap())
        });
        let sizes = case["carriers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|raw| raw.as_str().unwrap().len());
        let cost = class.cost(sizes).unwrap();
        assert_eq!(
            cost,
            RateCost {
                messages: case["messages"].as_u64().unwrap(),
                bytes: case["bytes"].as_u64().unwrap()
            },
            "{}",
            case["name"]
        );
    }
}

#[test]
fn maximum_default_payload_fits_after_fragment_encoding() {
    for fragment_bytes in [4, 8, 128, 512, 768, 1024, MAX_PAYLOAD_BYTES as u32] {
        let mut limits = crate::negotiation::Limits::host();
        limits.max_fragment_bytes = fragment_bytes;
        let wire = Wire {
            version: MAX_WIRE_VERSION,
            limits: limits.fit_rate(),
        };
        let max_payload = wire.limits.max_message_bytes as usize;
        for character in ['"', '\\', '\0', 'é', '😀'] {
            let mut envelope = Envelope {
                version: MAX_WIRE_VERSION,
                session: "f".repeat(64),
                connection: "f".repeat(64),
                subclient: 0,
                bundle: "b".repeat(MAX_IDENTIFIER_BYTES),
                generation: u64::MAX,
                channel: "c".repeat(MAX_IDENTIFIER_BYTES),
                schema: u16::MAX,
                sequence: u64::MAX,
                world_epoch: u64::MAX,
                payload: vec![Scalar::Text(String::new())],
            };
            let overhead = serde_json::to_vec(&envelope.payload).unwrap().len();
            let width = serde_json::to_string(&character.to_string()).unwrap().len() - 2;
            let text = character
                .to_string()
                .repeat((max_payload - overhead) / width);
            let padding = max_payload - overhead - text.len() / character.len_utf8() * width;
            envelope.payload = vec![Scalar::Text(text + &"a".repeat(padding))];
            assert_eq!(
                serde_json::to_vec(&envelope.payload).unwrap().len(),
                max_payload
            );
            let carriers = wire::encode(&envelope, &wire).unwrap();
            RateClass::Data.cost(carriers.iter().map(Vec::len)).unwrap();
        }
    }
}

#[test]
fn permanent_batches_spend_no_credit_and_refill_remains_bounded() {
    let mut rate = RateLimit::new(0);
    let original = rate.clone();
    assert!(
        rate.charge_all(vec![0; MAX_MESSAGES_PER_SECOND as usize + 1], 0)
            .unwrap_err()
            .is::<BurstExceeded>()
    );
    assert_eq!(rate, original);
    let sizes = std::iter::repeat_n(
        MAX_PAYLOAD_BYTES,
        MAX_BURST_BYTES as usize / MAX_PAYLOAD_BYTES,
    );
    rate.charge_all(sizes, 0).unwrap();
    assert!(rate.charge(1, 0).unwrap_err().is::<RateExceeded>());
    rate.charge(MAX_BYTES_PER_SECOND as usize, 1000).unwrap();
    assert!(rate.charge(1, 1000).is_err());
}
