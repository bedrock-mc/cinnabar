use protocol::{
    BedrockSession, FormButtonImage, ModalFormResponseSelection, ServerFormModel, UiEvent,
    WorldEvent, decode_batch, into_world_event, modal_form_submit_response,
};
use valentine::bedrock::version::v1_26_51::{McpePacketData, ModalFormRequestPacket};

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
                ModalFormRequestPacket {
                    form_id: 7,
                    form_uijson: form.to_string(),
                }
                .into(),
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
            let session = BedrockSession { shield_item_id: 0 };
            let packet = modal_form_submit_response(
                event.form_id,
                ModalFormResponseSelection::ButtonIndex((count - 1) as u32),
            );
            let bytes = protocol::encode(&packet, &session).unwrap();
            let decoded = decode_batch(bytes.to_vec().into(), &session).unwrap();
            let McpePacketData::ModalFormResponsePacket(response) = &decoded[0].data else {
                panic!("form response");
            };
            assert_eq!(response.form_id, event.form_id);
            assert_eq!(
                response.json_response.as_deref(),
                Some((count - 1).to_string().as_str())
            );
        }
    }
}
