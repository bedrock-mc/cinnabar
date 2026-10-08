use protocol::ServerDisconnectEvent;

/// Selects the first non-empty server-authored disconnect description.
pub(super) fn disconnect_display_reason(disconnect: &ServerDisconnectEvent) -> Option<String> {
    disconnect
        .message
        .as_deref()
        .filter(|message| !message.trim().is_empty())
        .or_else(|| {
            disconnect
                .filtered_message
                .as_deref()
                .filter(|message| !message.trim().is_empty())
        })
        .or_else(|| (!disconnect.reason.trim().is_empty()).then_some(disconnect.reason.as_str()))
        .map(str::to_owned)
}
