use std::{collections::VecDeque, sync::Arc};

pub const MAX_CHAT_MESSAGES: usize = 512;
pub const MAX_CHAT_RETAINED_BYTES: usize = 1_048_576;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChatMessageKind {
    Chat,
    System,
    Whisper,
    Announcement,
    Translation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChatMessage {
    pub fifo_sequence: u64,
    pub received_millis: u64,
    pub kind: ChatMessageKind,
    pub source: Option<Arc<str>>,
    pub message: Arc<str>,
    pub parameters: Arc<[Arc<str>]>,
}

impl ChatMessage {
    pub fn retained_bytes(&self) -> usize {
        self.source.as_ref().map_or(0, |value| value.len())
            + self.message.len()
            + self
                .parameters
                .iter()
                .map(|value| value.len())
                .sum::<usize>()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChatApplyResult {
    Applied { evicted: usize },
    RejectedStaleSequence,
    RejectedTooLarge,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChatViewNode {
    pub ordinal: u16,
    pub source_sequence: u64,
    pub source: Option<Arc<str>>,
    pub text: Arc<str>,
}

#[derive(Clone, Debug, Default)]
pub struct ChatStore {
    messages: VecDeque<ChatMessage>,
    retained_bytes: usize,
    last_sequence: Option<u64>,
}

impl ChatStore {
    pub fn messages(&self) -> &VecDeque<ChatMessage> {
        &self.messages
    }

    pub const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    pub const fn last_sequence(&self) -> Option<u64> {
        self.last_sequence
    }

    pub fn clear(&mut self) {
        self.messages.clear();
        self.retained_bytes = 0;
        self.last_sequence = None;
    }

    pub fn push(&mut self, message: ChatMessage) -> ChatApplyResult {
        if self
            .last_sequence
            .is_some_and(|last| message.fifo_sequence <= last)
        {
            return ChatApplyResult::RejectedStaleSequence;
        }
        let message_bytes = message.retained_bytes();
        if message_bytes > MAX_CHAT_RETAINED_BYTES {
            return ChatApplyResult::RejectedTooLarge;
        }

        let mut evicted = 0;
        while self.messages.len() >= MAX_CHAT_MESSAGES
            || self.retained_bytes + message_bytes > MAX_CHAT_RETAINED_BYTES
        {
            let Some(removed) = self.messages.pop_front() else {
                break;
            };
            self.retained_bytes = self.retained_bytes.saturating_sub(removed.retained_bytes());
            evicted += 1;
        }
        self.retained_bytes += message_bytes;
        self.last_sequence = Some(message.fifo_sequence);
        self.messages.push_back(message);
        ChatApplyResult::Applied { evicted }
    }

    /// Produces stable, read-only rows without exposing the mutable store to draw code.
    pub fn view_nodes(&self) -> Box<[ChatViewNode]> {
        self.messages
            .iter()
            .enumerate()
            .map(|(ordinal, message)| ChatViewNode {
                ordinal: u16::try_from(ordinal).expect("chat count is bounded below u16::MAX"),
                source_sequence: message.fifo_sequence,
                source: message.source.clone(),
                text: Arc::clone(&message.message),
            })
            .collect::<Vec<_>>()
            .into_boxed_slice()
    }
}
