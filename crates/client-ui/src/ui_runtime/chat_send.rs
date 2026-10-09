//! The outbound chat queue: enqueue, packet projection and send acknowledgement.

use std::{collections::VecDeque, sync::Arc};

use protocol::{ChatPacketError, Packet, chat_input_packet};
use ui::{ChatSendError, ChatSendRequest};

use super::UiRuntime;

impl UiRuntime {
    pub fn pending_chat_sends(&self) -> &VecDeque<ChatSendRequest> {
        self.chat_sends.pending()
    }

    pub const fn dropped_unsent_chat_messages(&self) -> u64 {
        self.dropped_unsent_chat_messages
    }

    pub fn set_chat_identity(&mut self, source_name: Arc<str>, xuid: Arc<str>) {
        self.chat_source_name = source_name;
        self.chat_xuid = xuid;
    }

    pub fn set_chat_source_name(&mut self, source_name: Arc<str>) {
        self.chat_source_name = source_name;
    }

    pub fn queue_chat_send(&mut self, now_millis: u64) -> Result<ChatSendRequest, ChatSendError> {
        let message = self.chat_editor.as_str();
        let request = self.chat_sends.push(self.session_id, message, now_millis)?;
        self.chat_history.push(Arc::clone(&request.message));
        self.chat_editor.clear();
        self.chat_autocomplete.clear();
        self.pending_chat_autocomplete_request = None;
        Ok(request)
    }

    /// Queues exactly `message` as a sent line, leaving the editor's draft untouched.
    pub fn queue_chat_message(
        &mut self,
        message: &str,
        now_millis: u64,
    ) -> Result<ChatSendRequest, ChatSendError> {
        let request = self.chat_sends.push(self.session_id, message, now_millis)?;
        self.chat_history.push(Arc::clone(&request.message));
        Ok(request)
    }

    /// The first queued message not yet handed to the transport.
    pub fn next_chat_packet(&self) -> Result<Option<(u64, Packet)>, ChatPacketError> {
        self.chat_sends
            .pending()
            .get(self.in_flight_chat_sends)
            .map(|request| {
                chat_input_packet(&self.chat_source_name, &self.chat_xuid, &request.message)
                    .map(|packet| (request.sequence, packet))
            })
            .transpose()
    }

    pub fn confirm_chat_send(&mut self, sequence: u64) -> bool {
        self.chat_sends.confirm_front(sequence)
    }

    /// Queued messages handed to the transport and awaiting its acknowledgement.
    pub const fn in_flight_chat_sends(&self) -> usize {
        self.in_flight_chat_sends
    }

    pub fn mark_chat_send_enqueued(&mut self, session: u64, sequence: u64) -> bool {
        if session != self.session_id
            || self
                .chat_sends
                .pending()
                .get(self.in_flight_chat_sends)
                .is_none_or(|request| request.session != session || request.sequence != sequence)
        {
            return false;
        }
        self.in_flight_chat_sends += 1;
        true
    }

    /// Acknowledgements arrive in send order, so only the oldest in-flight message confirms.
    pub fn acknowledge_chat_send(&mut self, session: u64, sequence: u64) -> bool {
        if self.in_flight_chat_sends == 0
            || session != self.session_id
            || !self.confirm_chat_send(sequence)
        {
            return false;
        }
        self.in_flight_chat_sends -= 1;
        true
    }

    /// A failed write leaves every unacknowledged message queued for a later flush.
    pub fn fail_chat_send(&mut self, session: u64, sequence: u64) -> bool {
        if session != self.session_id
            || !self
                .chat_sends
                .pending()
                .iter()
                .take(self.in_flight_chat_sends)
                .any(|request| request.sequence == sequence)
        {
            return false;
        }
        self.in_flight_chat_sends = 0;
        true
    }
}
