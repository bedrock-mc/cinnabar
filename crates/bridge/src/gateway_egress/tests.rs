use super::*;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Corpus {
    schema: u32,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    address: String,
    public: bool,
}

/// Both languages read the same generated boundaries, including every mapped IPv4 counterpart.
#[test]
fn gateway_egress_matches_shared_address_boundary_corpus() {
    let corpus: Corpus = serde_json::from_str(include_str!(
        "../../../../core/internal/bridgecontract/testdata/gateway-egress.json"
    ))
    .unwrap();
    assert_eq!(corpus.schema, 1);
    assert!(!corpus.cases.is_empty());
    for case in corpus.cases {
        let address = case.address.parse().unwrap();
        assert_eq!(
            public_gateway_upstream(address),
            case.public,
            "gateway policy differs for {}",
            case.address
        );
    }
}
