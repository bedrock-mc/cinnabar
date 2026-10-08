//! Client-directed transfer command identity shared by UI and transport.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FastTransferAction {
    TransferSm3,
}

impl FastTransferAction {
    /// Recognizes the opt-in transfer witness command.
    pub fn classify(message: &str) -> Option<Self> {
        (message == "/transfer sm3").then_some(Self::TransferSm3)
    }

    /// Formats the named action observation only once its packet has reached the socket.
    pub fn marker(
        self,
        marker_name: &str,
        session_generation: u64,
        action_ordinal: u64,
        sent_unix_ms: u64,
    ) -> String {
        let command = match self {
            Self::TransferSm3 => "/transfer sm3",
        };
        format!(
            "{marker_name}={}",
            serde_json::json!({
                "schema": "rust-mcbe-fast-transfer-action-v1",
                "kind": "command_sent",
                "session_generation": session_generation,
                "action_ordinal": action_ordinal,
                "command": command,
                "sent_unix_ms": sent_unix_ms,
            })
        )
    }
}
