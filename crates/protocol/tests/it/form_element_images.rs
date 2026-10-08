use protocol::{
    FormButtonImage, MenuElement, ServerFormModel, UiEvent, WorldEvent, into_world_event,
};
use valentine::bedrock::version::v1_26_51::ModalFormRequestPacket;

fn model(json: serde_json::Value) -> ServerFormModel {
    let event = into_world_event(
        ModalFormRequestPacket {
            form_id: 7,
            form_uijson: json.to_string(),
        }
        .into(),
        0,
    )
    .unwrap();
    let Some(WorldEvent::Ui(UiEvent::Form(event))) = event else {
        panic!("form request");
    };
    event.model
}

#[test]
fn both_button_representations_preserve_image_alignment_and_selection_indices() {
    for representation in ["buttons", "elements"] {
        let mut buttons = serde_json::json!([
            {"text":"absent"},
            {"text":"null", "image":null},
            {"text":"path", "image":{"type":"path","data":"textures/items/apple"}},
            {"text":"url", "image":{"type":"url","data":"https://example.invalid/icon.png"}}
        ]);
        if representation == "elements" {
            for button in buttons.as_array_mut().unwrap() {
                button["type"] = serde_json::json!("button");
            }
        }
        let mut form = serde_json::json!({"type":"form", "title":"Menu"});
        form[representation] = buttons;
        let ServerFormModel::TextMenu(menu) = model(form) else {
            panic!("supported {representation} menu");
        };
        assert_eq!(
            menu.buttons
                .iter()
                .map(|text| text.as_ref())
                .collect::<Vec<_>>(),
            ["absent", "null", "path", "url"]
        );
        assert_eq!(menu.button_images.len(), 4);
        assert_eq!(menu.button_images[0], None);
        assert_eq!(menu.button_images[1], None);
        assert!(
            matches!(&menu.button_images[2], Some(FormButtonImage::Path(path)) if path.as_ref() == "textures/items/apple")
        );
        assert!(
            matches!(&menu.button_images[3], Some(FormButtonImage::Url(url)) if url.as_ref() == "https://example.invalid/icon.png")
        );
        assert_eq!(menu.omitted_images, 2);
    }
}

#[test]
fn decorated_menu_preserves_button_image_without_counting_decoration_as_answer() {
    let ServerFormModel::ElementMenu(menu) = model(serde_json::json!({
        "type":"form", "elements":[
            {"type":"header", "text":"Header"},
            {"type":"button", "text":"Icon", "image":{"type":"path","data":"textures/items/apple"}},
            {"type":"divider"},
            {"type":"button", "text":"Plain"}
        ]
    })) else {
        panic!("decorated menu");
    };
    assert_eq!(menu.button_count(), 2);
    assert!(
        matches!(&menu.elements[1], MenuElement::Button { image: Some(FormButtonImage::Path(path)), .. } if path.as_ref() == "textures/items/apple")
    );
    assert!(matches!(
        &menu.elements[3],
        MenuElement::Button { image: None, .. }
    ));
}
