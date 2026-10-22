//! Command text attached to an explicit Send button in a chat message.

use json_ui::{HitKind, HitRegion};

/// A message action never falls back to sending an unrelated editor draft.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Submission<'a> {
    Draft,
    Command(&'a str),
    Ignore,
}

/// Reads the overlapping sibling chat field of a message's Send button.
/// Only Send outside message history may fall back to the main editor draft.
pub(crate) fn linked_command<'a>(
    button: &HitRegion,
    regions: &'a [HitRegion],
    message_local: bool,
) -> Submission<'a> {
    let missing = if message_local {
        Submission::Ignore
    } else {
        Submission::Draft
    };
    let Some((parent, _)) = button.key.rsplit_once('/') else {
        return missing;
    };
    let center = [
        button.rect.x + button.rect.w / 2.0,
        button.rect.y + button.rect.h / 2.0,
    ];
    let mut fields = regions.iter().filter(|field| {
        field.kind == HitKind::EditBox
            && field
                .key
                .rsplit_once('/')
                .is_some_and(|(scope, _)| scope == parent)
            && field.rect.x <= center[0]
            && center[0] < field.rect.x + field.rect.w
            && field.rect.y <= center[1]
            && center[1] < field.rect.y + field.rect.h
            && field
                .widget
                .edit
                .as_ref()
                .is_some_and(|edit| edit.name.as_deref() == Some("#message_text_box"))
    });
    let Some(field) = fields.next() else {
        return missing;
    };
    if !field.enabled || fields.next().is_some() {
        return Submission::Ignore;
    }
    field
        .widget
        .edit
        .as_ref()
        .and_then(|edit| command(&edit.text))
        .map_or(Submission::Ignore, Submission::Command)
}

/// Accepts one bounded command, including a quoted UI string or leading spaces.
fn command(text: &str) -> Option<&str> {
    let text = text
        .strip_prefix('"')
        .and_then(|text| text.strip_suffix('"'))
        .unwrap_or(text);
    let text = text.trim_start_matches(' ');
    (text.starts_with('/')
        && text.len() > 1
        && text.len() <= ui::MAX_CHAT_INPUT_BYTES
        && !text.chars().any(char::is_control))
    .then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A laid-out field and button sharing the same name-sized rectangle.
    fn pair(parent: &str, text: &str) -> [HitRegion; 2] {
        let name = parent.replace('/', "_");
        let reference = format!("fixture.{name}");
        let document = serde_json::json!({"namespace":"fixture",(name):{
        "type":"panel","size":[40,10],"controls":[
            {"editor":{"type":"edit_box","size":[40,10],
                "text_box_name":"#message_text_box","text_control":"command",
                "controls":[{"command":{"type":"label","size":[0,0],"visible":false,
                    "text":"#command","bindings":[{"binding_name":"#command"}]}}]}},
            {"submit":{"type":"button","size":[40,10],"button_mappings":[{
                "from_button_id":"button.menu_select","to_button_id":"button.send",
                "mapping_type":"pressed"}]}}
        ]}});
        let mut catalog = json_ui::Catalog::default();
        catalog.overlay_text("ui/fixture.json", &document.to_string());
        let context = json_ui::Context::default();
        let resolved = json_ui::resolve(&catalog, &reference, &context)
            .control
            .unwrap();
        let mut data = json_ui::DataSource::new();
        data.set_global("#command", json_ui::Scalar::Text(text.into()));
        let bound = json_ui::bind(&resolved, &data, &json_ui::EmptyLibrary);
        let env = json_ui::LayoutEnv {
            text: &crate::test_support::FixedText,
            textures: &crate::test_support::NoTextures,
        };
        let laid = json_ui::layout(&bound, [40.0, 10.0], &env);
        let regions = json_ui::hit_regions(&laid);
        [
            regions
                .iter()
                .find(|r| r.kind == HitKind::Button)
                .unwrap()
                .clone(),
            regions
                .iter()
                .find(|r| r.kind == HitKind::EditBox)
                .unwrap()
                .clone(),
        ]
    }

    #[test]
    fn each_name_reads_its_own_command() {
        let first = pair("/chat/history/row0/name", "\"/view first 0\"");
        let second = pair("/chat/history/row1/name", " /view second 1");
        let regions = [first[1].clone(), second[1].clone()];
        assert_eq!(
            linked_command(&first[0], &regions, true),
            Submission::Command("/view first 0")
        );
        assert_eq!(
            linked_command(&second[0], &regions, true),
            Submission::Command("/view second 1")
        );
        assert_eq!(
            linked_command(&first[0], &regions[1..], true),
            Submission::Ignore
        );
    }

    #[test]
    fn main_send_and_disabled_or_ambiguous_fields_are_not_overridden() {
        let [button, mut field] = pair("/chat/bottom", "/typed draft");
        field.rect.x = 60.0;
        assert_eq!(
            linked_command(&button, &[field.clone()], false),
            Submission::Draft
        );
        field.rect.x = 0.0;
        field.enabled = false;
        assert_eq!(
            linked_command(&button, &[field.clone()], true),
            Submission::Ignore
        );
        field.enabled = true;
        assert_eq!(
            linked_command(&button, &[field.clone(), field], true),
            Submission::Ignore
        );
        let [button, field] = pair("invalid", "/");
        assert_eq!(linked_command(&button, &[field], true), Submission::Ignore);
    }

    #[test]
    fn malformed_or_oversized_commands_are_rejected() {
        for text in ["", "/", "ordinary text", "/view\n/say other", "/view\0"] {
            assert_eq!(command(text), None);
        }
        assert_eq!(
            command(&format!("/{}", "x".repeat(ui::MAX_CHAT_INPUT_BYTES))),
            None
        );
    }
}
