use experience_sdk::{
    entry::server,
    server::{Callback, Experience, GuestError, PlayerId},
};

use crate::shared::{Ping, Pong};

struct Server;

#[server]
impl Experience for Server {
    /// Replies through the delivery path declared by Pong.
    #[on_message]
    fn ping(ctx: &Callback, player: PlayerId, ping: Ping) -> Result<(), GuestError> {
        ctx.send(
            &player,
            &Pong {
                request: ping.request,
            },
        )
    }
}
