//! Acknowledges a server world view through the declared typed channel.
use experience_sdk::{
    Value,
    client::{self, ClientPart},
};
experience_sdk::include_declarations!();

struct Client;

impl ClientPart for Client {
    fn init() {}

    fn dispatch(channel: String, record: Vec<Value>) {
        if channel == channels::VIEW.id && matches!(record.as_slice(), [Value::Choice(_)]) {
            let _ = client::send(&channels::ACK, &record);
        }
    }
}

experience_sdk::export_client_part!(Client);
