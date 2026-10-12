use std::collections::BTreeMap;

use json_ui::{CollectionItem, DataSource, Scalar};
use server_experience::screen;

use super::data_source;

#[test]
fn bound_values_and_rows_reach_the_engine_bindings() {
    let mut modal = screen::Modal::default();
    modal.set_value("#title".into(), screen::Value::Text("§eME".into()));
    modal.set_value(
        "#color".into(),
        screen::Value::Numbers(vec![1.0, 0.5, 0.0, 1.0]),
    );
    modal.set_collection(
        "items".into(),
        vec![BTreeMap::from([
            ("#count".to_owned(), screen::Value::Integer(64)),
            ("#shown".to_owned(), screen::Value::Bool(false)),
        ])],
    );
    let mut expected = DataSource::new();
    expected.set_global("#title", Scalar::Text("§eME".into()));
    expected.set_global(
        "#color",
        Scalar::Json(serde_json::json!([1.0, 0.5, 0.0, 1.0])),
    );
    expected.set_collection(
        "items",
        vec![
            CollectionItem::default()
                .with("#count", Scalar::Int(64))
                .with("#shown", Scalar::Bool(false)),
        ],
    );
    assert_eq!(data_source(&modal), expected);
}
