use experience_sdk::{
    client::{self, ClientPart, ui},
    entry::client,
    message::Registry,
};

use crate::shared::{Ping, Pong};

struct Client;

#[client]
impl ClientPart for Client {
    fn registry() -> Option<&'static Registry> {
        Some(&crate::shared::REGISTRY)
    }

    fn init() {
        client::send(&Ping { request: 1 }).expect("send moon ping");
    }

    /// Draws the server's typed reply.
    #[on_message]
    fn pong(pong: Pong) {
        ui::set_widget("cosmic.moon", &format!("☾ Cosmic reply {}", pong.request))
            .expect("draw moon label");
    }
}
