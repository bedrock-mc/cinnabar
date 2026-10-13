use super::*;

#[test]
fn rawtext_translation_arguments_preserve_unmarked_and_unknown_keys() {
    let entries = [("menu.play", "Play"), ("wrap", "%s")].map(|(key, value)| assets::LangEntry {
        key: key.into(),
        value: value.into(),
    });
    let bytes = assets::encode_lang_catalog([9; 32], [10; 32], &entries).unwrap();
    let mut runtime = UiRuntime::new(1);
    runtime.set_lang_catalog(Arc::new(
        assets::RuntimeLangCatalog::decode(&bytes).unwrap(),
    ));
    for argument in ["menu.play", "100% literal %menu.play", "%missing"] {
        let document = protocol::parse_raw_text(&format!(
            r#"{{"rawtext":[{{"translate":"wrap","with":["{argument}"]}}]}}"#
        ))
        .unwrap();
        assert_eq!(runtime.resolve_raw_text(&document).text, argument);
    }
}
