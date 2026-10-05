use super::{ClientHandshakeConfig, LoginPacket, encode_connection_request};
use crate::error::JolyneError;

pub(super) async fn prepare(config: &ClientHandshakeConfig) -> Result<LoginPacket, JolyneError> {
    let (chain, client_token) = if let Some(xbl) = &config.xbl_credentials {
        let mojang_chain = crate::auth::client::request_minecraft_chain(
            &config.identity_key,
            &xbl.token,
            &xbl.user_hash,
        )
        .await?;
        crate::auth::client::encode_with_mojang_chain(
            &config.identity_key,
            &config.display_name,
            config.uuid,
            &mojang_chain,
            config.skin.as_ref(),
        )?
    } else {
        crate::auth::client::generate_self_signed_chain(
            &config.identity_key,
            &config.display_name,
            config.uuid,
            config.skin.as_ref(),
        )?
    };
    Ok(LoginPacket {
        client_network_version: crate::valentine::PROTOCOL_VERSION,
        connection_request: encode_connection_request(&chain, &client_token),
    })
}
