use super::{locale_generated as locale, unicode_generated as unicode};
use crate::login::client_data::raw::schema_generated;

/// Rust source pins share one actual generated identity; Go's owning test verifies the actual data.
#[test]
fn generated_semantic_data_matches_the_selected_go_sources() {
    assert_eq!(
        [
            unicode::CLIENT_MODULE,
            unicode::CLIENT_VERSION,
            unicode::CLIENT_SUM
        ],
        [
            schema_generated::SOURCE_MODULE,
            schema_generated::SOURCE_VERSION,
            schema_generated::SOURCE_SUM
        ],
    );
    assert_eq!(
        [
            unicode::CLIENT_MODULE,
            unicode::CLIENT_VERSION,
            unicode::CLIENT_SUM,
            unicode::LOCALE_MODULE,
            unicode::LOCALE_VERSION,
            unicode::LOCALE_SUM,
            unicode::UNICODE_VERSION
        ],
        [
            locale::CLIENT_MODULE,
            locale::CLIENT_VERSION,
            locale::CLIENT_SUM,
            locale::LOCALE_MODULE,
            locale::LOCALE_VERSION,
            locale::LOCALE_SUM,
            locale::UNICODE_VERSION
        ],
    );
    let module = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../core/go.mod"));
    assert_eq!(
        module
            .lines()
            .filter(|line| {
                let line = line.split_once("//").map_or(*line, |(code, _)| code);
                let fields = line.split_whitespace().collect::<Vec<_>>();
                fields == [locale::LOCALE_MODULE, locale::LOCALE_VERSION]
                    || fields == ["require", locale::LOCALE_MODULE, locale::LOCALE_VERSION]
            })
            .count(),
        1,
        "generated locale source differs from the unique core requirement",
    );
    let sums = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../core/go.sum"));
    assert_eq!(
        sums.lines()
            .filter(|line| line.split_whitespace().eq([
                locale::LOCALE_MODULE,
                locale::LOCALE_VERSION,
                locale::LOCALE_SUM,
            ]))
            .count(),
        1,
        "generated locale checksum differs from core/go.sum",
    );
}
