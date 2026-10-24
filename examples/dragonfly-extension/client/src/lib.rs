//! Acknowledges a server world view through the declared typed channel.
use experience_sdk::{
    Value,
    client::{self, ClientPart},
    entry::client,
    message::{Message, ToClient, ToServer},
};
experience_sdk::include_declarations!();

struct View(u16);
struct Ack(u16);

macro_rules! choice_message {
    ($ty:ident, $channel:expr) => {
        impl Message for $ty {
            const CHANNEL: experience_sdk::Channel = $channel;
            /// Encodes the existing choice field after checking its bounds.
            fn encode(&self) -> Result<Vec<Value>, String> {
                let record = vec![Value::Choice(self.0)];
                experience_sdk::message::validate(&Self::CHANNEL, &record)?;
                Ok(record)
            }
            /// Requires the manifest's single bounded choice field.
            fn decode(record: Vec<Value>) -> Result<Self, String> {
                experience_sdk::message::validate(&Self::CHANNEL, &record)?;
                match record.as_slice() {
                    [Value::Choice(choice)] => Ok(Self(*choice)),
                    _ => Err("expected world choice".into()),
                }
            }
        }
    };
}
choice_message!(View, channels::VIEW);
choice_message!(Ack, channels::ACK);
impl ToClient for View {}
impl ToServer for Ack {}

struct Client;

#[client]
impl ClientPart for Client {
    fn init() {}

    /// Acknowledges the typed view without changing the Go example's wire format.
    #[on_message]
    fn view(view: View) {
        let _ = client::send(&Ack(view.0));
    }
}
