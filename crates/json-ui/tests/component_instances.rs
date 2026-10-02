//! Input writes must survive rebinding expanded and hoisted widget instances.

use json_ui::{
    ButtonInput, Catalog, CatalogLibrary, CollectionItem, Context, DataSource, Dispatcher,
    FactoryItem, InputMode, LayoutEnv, TextMeasure, TextureMeta, TextureSource, ViewState, bind,
    hit_regions, layout, resolve,
};
use serde_json::json;

struct Empty;
impl TextMeasure for Empty {
    fn extent(&self, _: &str) -> [f64; 2] {
        [0.0; 2]
    }
}
impl TextureSource for Empty {
    fn texture(&self, _: &str) -> Option<TextureMeta> {
        None
    }
}

/// Resolve a small pack whose factory and grid share an edit-box template.
fn catalog() -> Catalog {
    let pack = json!({"namespace":"test",
        "edit": {"type":"edit_box","size":[40,20],"max_length":20,
            "text_control":"text","place_holder_control":"placeholder",
            "button_mappings":[{"from_button_id":"button.menu_select","to_button_id":"button.edit","mapping_type":"pressed"}],
            "controls":[{"text":{"type":"label","text":"","size":[40,20]}},
                {"placeholder":{"type":"label","text":"Hint","size":[40,20]}}]},
        "collection": {"type":"panel","size":[200,200],"collection_name":"rows",
            "factory":{"name":"collection","control_name":"test.edit"}},
        "grid": {"type":"grid","size":[200,200],"collection_name":"rows",
            "grid_dimensions":[1,2],"grid_item_template":"test.edit"},
        "root_factory": {"type":"factory","size":[200,200],"factory":{"name":"feed","control_ids":{"edit":"test.edit"}}},
        "hoisted": {"type":"panel","size":[200,200],"controls":[
            {"factory":{"type":"factory","factory":{"name":"feed","control_ids":{"edit":"test.edit"}}}}]}
    });
    let bytes = serde_json::to_vec(&pack).unwrap();
    Catalog::from_files([
        ("ui/_global_variables.json", b"{}".as_slice()),
        (
            "ui/_ui_defs.json",
            br#"{"ui_defs":["ui/test.json"]}"#.as_slice(),
        ),
        ("ui/test.json", bytes.as_slice()),
    ])
    .unwrap()
}

#[test]
fn expanded_widget_component_writes_survive_rebinding() {
    let catalog = catalog();
    let context = Context::desktop();
    let library = CatalogLibrary {
        catalog: &catalog,
        context: &context,
    };
    let env = LayoutEnv {
        text: &Empty,
        textures: &Empty,
    };
    for name in ["collection", "grid", "hoisted", "root_factory"] {
        let root = resolve(&catalog, &format!("test.{name}"), &context)
            .control
            .unwrap();
        let mut data = DataSource::new();
        data.set_collection(
            "rows",
            vec![CollectionItem::default(), CollectionItem::default()],
        );
        data.set_factory("feed", vec![FactoryItem::new("edit", 0.0).named("renamed")]);
        let bound = bind(&root, &data, &library);
        let regions = hit_regions(&layout(&bound, [200.0, 200.0], &env));
        let edit = regions
            .iter()
            .find(|region| region.widget.edit.is_some())
            .unwrap();
        let mut view = ViewState {
            focused: Some(edit.key.clone()),
            ..Default::default()
        };
        let mut dispatcher = Dispatcher::default();
        dispatcher.button(
            &regions,
            &mut view,
            ButtonInput {
                id: "button.menu_select",
                down: true,
                point: Some([edit.rect.x + 1.0, edit.rect.y + 1.0]),
                mode: InputMode::Mouse,
                now: 0.0,
            },
        );
        assert_eq!(
            view.components.selected(),
            Some(edit.key.as_str()),
            "{name}"
        );
        data.set_components(view.components);
        let rebound = bind(&root, &data, &library);
        let regions = hit_regions(&layout(&rebound, [200.0, 200.0], &env));
        let rebound_edit = regions
            .iter()
            .find(|region| region.key == edit.key)
            .unwrap();
        assert_eq!(rebound_edit.widget.edit.as_ref().unwrap().text, "");
        let placeholder = rebound
            .find(&|control| control.name == "placeholder")
            .unwrap();
        assert_eq!(
            placeholder.properties.get("visible"),
            Some(&json!(false)),
            "{name}: {}",
            edit.key
        );
    }
}
