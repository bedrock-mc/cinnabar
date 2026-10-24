use experience_sdk::{
    Value,
    entry::server,
    message::Message,
    server::{Callback, Experience, GuestError, PlayerId, nodes},
};

use crate::shared::{Ping, Pong};

struct Server;

#[server]
impl Experience for Server {
    fn client_message(
        ctx: &Callback,
        player: PlayerId,
        channel: String,
        schema: u16,
        payload: Vec<Value>,
    ) -> Result<(), GuestError> {
        if channel != Ping::CHANNEL.id || schema != Ping::CHANNEL.schema {
            return Err(GuestError::Rejected("unknown message".into()));
        }
        let ping = Ping::decode(payload).map_err(GuestError::Rejected)?;
        let reply = Pong {
            request: ping.request,
        }
        .encode()
        .map_err(GuestError::Rejected)?;
        ctx.send_control(
            &player,
            Pong::CHANNEL.id,
            Pong::CHANNEL.schema,
            &nodes(reply),
        )
        .map_err(|error| GuestError::Failed(format!("send moon pong: {error:?}")))
    }
}
