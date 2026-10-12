use bedrock_protocol_1_26_51::packets::{self, ModalFormRequest};
use bedrock_protocol_1_26_51::wire;
use protocol::{
    FormButtonImage, ModalFormResponseSelection, ServerFormModel, UiEvent, WorldEvent,
    decode_batch, into_world_event, modal_form_submit_response,
};

#[test]
fn menus_above_256_preserve_every_button_image_and_last_wire_selection() {
    for count in [257usize, 300, 5_000] {
        for representation in ["buttons", "elements"] {
            let buttons: Vec<_> = (0..count)
                .map(|index| {
                    let mut button = serde_json::json!({
                        "text": format!("Choice {index}"),
                        "image": {"type": "url", "data": format!("https://example.invalid/{index}")}
                    });
                    if representation == "elements" {
                        button["type"] = serde_json::json!("button");
                    }
                    button
                })
                .collect();
            let mut form = serde_json::json!({"type":"form", "title":"Menu", "content":" "});
            form[representation] = serde_json::json!(buttons);
            let Some(WorldEvent::Ui(UiEvent::Form(event))) = into_world_event(
                protocol::Packet::new(ModalFormRequest {
                    form_id: wire::VarUInt(7),
                    form_ui_json: form.to_string().into(),
                }),
                0,
            )
            .unwrap() else {
                panic!("form request");
            };
            let ServerFormModel::TextMenu(menu) = event.model else {
                panic!("all {count} {representation} must be admitted");
            };
            assert_eq!(menu.buttons.len(), count);
            assert_eq!(menu.button_images.len(), count);
            for (index, (text, image)) in menu
                .buttons
                .iter()
                .zip(menu.button_images.iter())
                .enumerate()
            {
                assert_eq!(text.as_ref(), format!("Choice {index}"));
                assert!(
                    matches!(image, Some(FormButtonImage::Url(url)) if url.as_ref() == format!("https://example.invalid/{index}"))
                );
            }
            let packet = modal_form_submit_response(
                event.form_id,
                ModalFormResponseSelection::ButtonIndex((count - 1) as u32),
            );
            let bytes = protocol::encode(&packet).unwrap();
            let decoded = decode_batch(bytes.to_vec().into()).unwrap();
            let packets::Packet::ModalFormResponse(response) = &*decoded[0].data else {
                panic!("form response");
            };
            assert_eq!(response.form_id.0, event.form_id);
            assert_eq!(response.json_response, Some((count - 1).to_string().into()));
        }
    }
}
