use experience_sdk::{
    Value,
    client::{self, ClientPart, ui},
    entry::client,
    message::{Message, Registry},
};

use crate::shared::{Ping, Pong};

struct Client;

#[client]
impl ClientPart for Client {
    fn registry() -> Option<&'static Registry> {
        Some(&crate::shared::REGISTRY)
    }

    fn init() {
        client::send_message(&Ping { request: 1 }).expect("send moon ping");
    }

    fn dispatch(channel: String, record: Vec<Value>) {
        if channel == Pong::CHANNEL.id {
            let pong = Pong::decode(record).expect("decode moon pong");
            ui::set_widget("cosmic.moon", &format!("☾ Cosmic reply {}", pong.request))
                .expect("draw moon label");
        }
    }
}
