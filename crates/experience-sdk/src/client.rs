//! The client part: a component of the `server-bundle` world, which Cinnabar runs from the
//! Experience's `.cxb`, generated from `wit/client/client.wit`.
//!
//! Implement [`ClientPart`] on a type and export it with
//! [`export_client_part!`](crate::export_client_part). Records travel as [`Value`]s:
//! [`ClientPart::dispatch`] receives what the server half sends, and [`send`] sends a record on a
//! declared channel. The host's imports are [`ui`], [`input`], [`messaging`], [`scene`] and
//! [`media`]; each refuses what the bundle's grants do not allow.

/// Bindings generated from `wit/client/client.wit`.
pub mod bindings {
    wit_bindgen::generate!({
        path: "wit/client",
        world: "server-bundle",
        generate_all,
        pub_export_macro: true,
    });
}

pub use bindings::cinnabar::server_experience::{input, media, messaging, scene, ui};

use crate::{Channel, Value};

/// One client part, mirroring the exports of the `server-bundle` world.
///
/// Only [`ClientPart::init`] is required; the callbacks default to doing nothing.
pub trait ClientPart {
    /// Runs once, when the client part starts.
    fn init();

    /// Handles `dispatch`: the server half sent `record` on the to-client channel `channel`. The
    /// host has validated it against the channel's declaration.
    fn dispatch(_channel: String, _record: Vec<Value>) {}

    /// Handles `action`: the declared action `id` fired from a modal screen control while the
    /// modal has focus; `collection_index` is the control's row in its nearest collection.
    fn action(_id: String, _collection_index: Option<u32>) {}

    /// Handles `epoch`: the world epoch changed, such as on a dimension change, and the client
    /// part and its state live on. The server half resends what it needs to.
    fn epoch() {}

    /// Handles `modal-resized`: the open modal was first drawn, or its size changed (a window
    /// resize or a GUI scale change). `size` is what [`ui::modal_size`] now returns: the width
    /// and height the vanilla screen root lays out in, in GUI units, and the GUI scale.
    fn modal_resized(_size: ui::GuiSize) {}

    /// Handles `text-changed`: the user edited the modal's edit box whose `text_box_name` is
    /// `control`, a declared action, while the modal had focus; `text` is its whole text. Edits
    /// are coalesced, so this is the latest text, not each keystroke. [`ui::set_text`] sets a
    /// box's text without calling this.
    fn text_changed(_control: String, _text: String) {}

    /// Handles `secondary-action`: a secondary press (a right click) fired the declared action
    /// `id` from a modal screen control mapping `button.menu_secondary_select` to it, while the
    /// modal had focus; `collection_index` is as in [`ClientPart::action`].
    fn secondary_action(_id: String, _collection_index: Option<u32>) {}
}

/// Sends `record` on the to-server `channel`. The host checks it against the channel's
/// declaration and the session's message limit, and refuses it with the reason.
pub fn send(channel: &Channel, record: &[Value]) -> Result<(), String> {
    messaging::send(channel.id, channel.schema, &record_json(record))
}

/// `record` in the wire's JSON form, as `messaging.send` takes it.
fn record_json(record: &[Value]) -> Vec<u8> {
    serde_json::to_vec(record).expect("values always serialize")
}

/// The record that `dispatch` delivers in the wire's JSON form; `None` for anything else.
#[doc(hidden)]
pub fn record(json: &[u8]) -> Option<Vec<Value>> {
    serde_json::from_slice(json).ok()
}

/// Implements the generated [`bindings::Guest`] for `$ty` by delegating to its [`ClientPart`]
/// impl, then exports `$ty` as the component's `server-bundle` world.
#[macro_export]
macro_rules! export_client_part {
    ($ty:ident) => {
        impl $crate::client::bindings::Guest for $ty {
            fn init() {
                <$ty as $crate::client::ClientPart>::init()
            }

            fn dispatch(channel: ::std::string::String, record_json: ::std::vec::Vec<u8>) {
                // The host dispatches only records it validated, so nothing else arrives.
                if let ::core::option::Option::Some(record) = $crate::client::record(&record_json)
                {
                    <$ty as $crate::client::ClientPart>::dispatch(channel, record)
                }
            }

            fn action(id: ::std::string::String, collection_index: ::core::option::Option<u32>) {
                <$ty as $crate::client::ClientPart>::action(id, collection_index)
            }

            fn epoch() {
                <$ty as $crate::client::ClientPart>::epoch()
            }

            fn modal_resized(size: $crate::client::ui::GuiSize) {
                <$ty as $crate::client::ClientPart>::modal_resized(size)
            }

            fn text_changed(control: ::std::string::String, text: ::std::string::String) {
                <$ty as $crate::client::ClientPart>::text_changed(control, text)
            }

            fn secondary_action(
                id: ::std::string::String,
                collection_index: ::core::option::Option<u32>,
            ) {
                <$ty as $crate::client::ClientPart>::secondary_action(id, collection_index)
            }
        }

        $crate::client::bindings::export!($ty with_types_in $crate::client::bindings);
    };
}

#[cfg(test)]
mod tests {
    use super::{record, record_json};
    use crate::Value;

    /// Every kind of value in the wire's JSON form, as the host dispatches and parses records.
    const RECORD: &str = concat!(
        r#"[{"type":"bool","value":true},{"type":"integer","value":-7},"#,
        r#"{"type":"text","value":"cell"},{"type":"choice","value":2},"#,
        r#"{"type":"list","value":[{"type":"record","value":[{"type":"integer","value":1}]}]},"#,
        r#"{"type":"record","value":[]}]"#,
    );

    fn values() -> Vec<Value> {
        vec![
            Value::Bool(true),
            Value::Integer(-7),
            Value::Text("cell".to_owned()),
            Value::Choice(2),
            Value::List(vec![Value::Record(vec![Value::Integer(1)])]),
            Value::Record(Vec::new()),
        ]
    }

    #[test]
    fn records_travel_in_the_wire_json_form() {
        assert_eq!(record_json(&values()), RECORD.as_bytes());
        assert_eq!(record(RECORD.as_bytes()), Some(values()));
    }

    #[test]
    fn anything_but_a_record_is_none() {
        for json in [
            r#"{"type":"bool","value":true}"#,
            r#"[{"type":"float","value":1.5}]"#,
            r#"[{"type":"integer","value":1,"extra":0}]"#,
            r#"[{"type":"choice","value":-1}]"#,
            "not json",
        ] {
            assert_eq!(record(json.as_bytes()), None, "{json}");
        }
    }
}
