use super::merged_server_lang;
use resource_pack::test_support::{layered_pack_view, pack_view};

/// Admits an original English language file through the production pack reader.
fn english(text: String) -> resource_pack::LayeredPackView {
    pack_view(&[("texts/en_US.lang".into(), text.into_bytes())])
}

#[test]
fn ten_thousand_pack_translations_remain_available() {
    let text = (0..10_000)
        .map(|index| format!("test.key{index}=value{index}\n"))
        .collect();
    let view = english(text);
    let table = merged_server_lang(&view).expect("large language table");
    for index in 0..10_000 {
        assert_eq!(
            table.lookup(&format!("test.key{index}")),
            Some(format!("value{index}").as_str())
        );
    }
}

#[test]
fn long_pack_translation_fields_preserve_neighboring_definitions() {
    let key = format!("test.{}", "key".repeat(100));
    let value = "translation ".repeat(1000);
    let view = english(format!(
        "test.before=Before\n{key}={value}\ntest.after=After\n"
    ));
    let table = merged_server_lang(&view).expect("long language fields");
    assert_eq!(table.lookup(&key), Some(value.as_str()));
    assert_eq!(table.lookup("test.before"), Some("Before"));
    assert_eq!(table.lookup("test.after"), Some("After"));
}

#[test]
fn multi_megabyte_language_layers_keep_lower_keys_and_last_overrides() {
    let layers: Vec<_> = (0..3)
        .map(|layer| {
            let mut text = (0..5000)
                .map(|index| format!("test.layer{layer}.key{index}={}\n", "v".repeat(400)))
                .collect::<String>();
            text.push_str(&format!("test.shared=Layer {layer}\n"));
            vec![("texts/en_US.lang".into(), text.into_bytes())]
        })
        .collect();
    assert!(layers.iter().all(|layer| layer[0].1.len() > 2_000_000));
    let view = layered_pack_view(&layers.iter().map(Vec::as_slice).collect::<Vec<_>>());
    let table = merged_server_lang(&view).expect("all language layers");
    let value = "v".repeat(400);
    for layer in 0..3 {
        for index in [0, 4999] {
            assert_eq!(
                table.lookup(&format!("test.layer{layer}.key{index}")),
                Some(value.as_str())
            );
        }
    }
    assert_eq!(table.lookup("test.shared"), Some("Layer 2"));
}

#[test]
fn mixed_case_pack_keys_share_one_override_identity() {
    let view = english("Test.Title=First\ntest.TITLE=Last\nTest.Other=Preserved\n".into());
    let table = merged_server_lang(&view).expect("mixed-case language keys");
    assert_eq!(table.lookup("test.title"), Some("Last"));
    assert_eq!(table.lookup("Test.Title"), Some("Last"));
    assert_eq!(table.lookup("test.other"), Some("Preserved"));
}

#[test]
fn invalid_language_layer_preserves_other_pack_definitions() {
    let lower = vec![(
        "texts/en_US.lang".into(),
        b"test.lower=Lower\ntest.shared=Before".to_vec(),
    )];
    let invalid = vec![("texts/en_US.lang".into(), b"test.bad=\xff".to_vec())];
    let higher = vec![(
        "texts/en_US.lang".into(),
        b"\xef\xbb\xbftest.shared=After\r\ntest.higher=Higher=value\r\n".to_vec(),
    )];
    let view = layered_pack_view(&[&lower, &invalid, &higher]);
    let table = merged_server_lang(&view).expect("readable language layers");
    assert_eq!(table.lookup("test.lower"), Some("Lower"));
    assert_eq!(table.lookup("test.shared"), Some("After"));
    assert_eq!(table.lookup("test.higher"), Some("Higher=value"));
}
