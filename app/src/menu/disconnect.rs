use launcher::menu::disconnect::*;

#[cfg(test)]
mod tests {
    use {
        super::*,
        launcher::menu::disconnect::{DisconnectBody, SERVER_SENT_KEYS, describe},
    };
    #[test]
    fn network_session_failures_keep_known_server_language_keys() {
        let key = DisconnectBody::Key;
        for sent in SERVER_SENT_KEYS {
            let error = crate::runtime::network::session_failure_display(
                "Bedrock session failed: Server disconnected during login: Unknown",
                Some(&protocol::ServerDisconnectEvent {
                    reason: "Unknown".to_owned(),
                    message: Some((*sent).to_owned()),
                    filtered_message: None,
                }),
            );
            assert_eq!(describe(&error).body, key(sent));
        }
    }
}
