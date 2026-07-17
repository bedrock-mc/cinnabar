use std::{collections::VecDeque, sync::Arc};

pub const MAX_CHAT_MESSAGES: usize = 512;
pub const MAX_CHAT_RETAINED_BYTES: usize = 1_048_576;
pub const MAX_CHAT_INPUT_BYTES: usize = 512;
pub const MAX_CHAT_HISTORY: usize = 128;
pub const MAX_PENDING_CHAT_SENDS: usize = 64;
pub const MAX_CHAT_AUTOCOMPLETE: usize = 256;
pub const MAX_CHAT_AUTOCOMPLETE_BYTES: usize = 65_536;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChatEditorError {
    InvalidMaximum,
    InputTooLong { maximum: usize },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChatEditor {
    text: String,
    cursor: usize,
    selection_anchor: Option<usize>,
    maximum_bytes: usize,
}

impl ChatEditor {
    pub fn new(maximum_bytes: usize) -> Result<Self, ChatEditorError> {
        if maximum_bytes == 0 || maximum_bytes > MAX_CHAT_INPUT_BYTES {
            return Err(ChatEditorError::InvalidMaximum);
        }
        Ok(Self {
            text: String::with_capacity(maximum_bytes),
            cursor: 0,
            selection_anchor: None,
            maximum_bytes,
        })
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub fn bytes(&self) -> &[u8] {
        self.text.as_bytes()
    }

    pub const fn len_bytes(&self) -> usize {
        self.text.len()
    }

    pub const fn cursor_byte(&self) -> usize {
        self.cursor
    }

    pub fn selection(&self) -> Option<core::ops::Range<usize>> {
        self.selection_anchor
            .filter(|anchor| *anchor != self.cursor)
            .map(|anchor| anchor.min(self.cursor)..anchor.max(self.cursor))
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.selection_anchor = None;
    }

    pub fn insert(&mut self, value: &str) -> Result<(), ChatEditorError> {
        let selection = self.selection();
        let removed = selection.as_ref().map_or(0, core::ops::Range::len);
        let requested = self
            .text
            .len()
            .saturating_sub(removed)
            .saturating_add(value.len());
        if requested > self.maximum_bytes {
            return Err(ChatEditorError::InputTooLong {
                maximum: self.maximum_bytes,
            });
        }
        if let Some(range) = selection {
            self.text.replace_range(range.clone(), value);
            self.cursor = range.start + value.len();
        } else {
            self.text.insert_str(self.cursor, value);
            self.cursor += value.len();
        }
        self.selection_anchor = None;
        Ok(())
    }

    pub fn move_left(&mut self) {
        if let Some(range) = self.selection() {
            self.cursor = range.start;
        } else {
            self.cursor = previous_boundary(&self.text, self.cursor);
        }
        self.selection_anchor = None;
    }

    pub fn move_right(&mut self) {
        if let Some(range) = self.selection() {
            self.cursor = range.end;
        } else {
            self.cursor = next_boundary(&self.text, self.cursor);
        }
        self.selection_anchor = None;
    }

    pub fn select_left(&mut self) {
        let anchor = self.selection_anchor.get_or_insert(self.cursor);
        self.cursor = previous_boundary(&self.text, self.cursor);
        if self.cursor == *anchor {
            self.selection_anchor = None;
        }
    }

    pub fn select_right(&mut self) {
        let anchor = self.selection_anchor.get_or_insert(self.cursor);
        self.cursor = next_boundary(&self.text, self.cursor);
        if self.cursor == *anchor {
            self.selection_anchor = None;
        }
    }

    pub fn move_home(&mut self, selecting: bool) {
        self.move_to(0, selecting);
    }

    pub fn move_end(&mut self, selecting: bool) {
        self.move_to(self.text.len(), selecting);
    }

    pub fn backspace(&mut self) {
        if self.delete_selection() {
            return;
        }
        let start = previous_boundary(&self.text, self.cursor);
        self.text.replace_range(start..self.cursor, "");
        self.cursor = start;
    }

    pub fn delete(&mut self) {
        if self.delete_selection() {
            return;
        }
        let end = next_boundary(&self.text, self.cursor);
        self.text.replace_range(self.cursor..end, "");
    }

    fn move_to(&mut self, cursor: usize, selecting: bool) {
        if selecting {
            self.selection_anchor.get_or_insert(self.cursor);
        } else {
            self.selection_anchor = None;
        }
        self.cursor = cursor;
        if self.selection_anchor == Some(self.cursor) {
            self.selection_anchor = None;
        }
    }

    fn delete_selection(&mut self) -> bool {
        let Some(range) = self.selection() else {
            return false;
        };
        self.text.replace_range(range.clone(), "");
        self.cursor = range.start;
        self.selection_anchor = None;
        true
    }
}

fn previous_boundary(value: &str, cursor: usize) -> usize {
    value[..cursor]
        .char_indices()
        .next_back()
        .map_or(0, |(index, _)| index)
}

fn next_boundary(value: &str, cursor: usize) -> usize {
    value[cursor..]
        .char_indices()
        .nth(1)
        .map_or(value.len(), |(index, _)| cursor + index)
}

#[derive(Clone, Debug, Default)]
pub struct ChatHistory {
    entries: VecDeque<Arc<str>>,
    cursor: Option<usize>,
}

impl ChatHistory {
    pub fn entries(&self) -> &VecDeque<Arc<str>> {
        &self.entries
    }

    pub fn push(&mut self, entry: Arc<str>) -> bool {
        if entry.is_empty() || entry.len() > MAX_CHAT_INPUT_BYTES {
            return false;
        }
        self.cursor = None;
        if self.entries.back().is_some_and(|last| last == &entry) {
            return true;
        }
        if self.entries.len() >= MAX_CHAT_HISTORY {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
        true
    }

    pub fn older(&mut self) -> Option<Arc<str>> {
        if self.entries.is_empty() {
            return None;
        }
        let index = self
            .cursor
            .map_or(self.entries.len() - 1, |index| index.saturating_sub(1));
        self.cursor = Some(index);
        self.entries.get(index).cloned()
    }

    pub fn newer(&mut self) -> Option<Arc<str>> {
        let index = self.cursor?;
        if index + 1 >= self.entries.len() {
            self.cursor = None;
            return None;
        }
        self.cursor = Some(index + 1);
        self.entries.get(index + 1).cloned()
    }

    pub fn clear_navigation(&mut self) {
        self.cursor = None;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChatRateLimit {
    maximum_messages: usize,
    window_millis: u64,
}

impl ChatRateLimit {
    pub const fn new(maximum_messages: usize, window_millis: u64) -> Option<Self> {
        if maximum_messages == 0 || window_millis == 0 {
            return None;
        }
        Some(Self {
            maximum_messages,
            window_millis,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChatSendRequest {
    pub session: u64,
    pub sequence: u64,
    pub message: Arc<str>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChatSendError {
    InvalidCapacity,
    EmptyMessage,
    MessageTooLong,
    WrongSession { expected: u64, actual: u64 },
    NonMonotonicClock { previous: u64, actual: u64 },
    RateLimited,
    QueueFull,
}

#[derive(Clone, Debug)]
pub struct ChatSendQueue {
    capacity: usize,
    rate: ChatRateLimit,
    session: Option<u64>,
    next_sequence: u64,
    accepted_millis: VecDeque<u64>,
    pending: VecDeque<ChatSendRequest>,
    last_millis: Option<u64>,
}

impl ChatSendQueue {
    pub fn new(capacity: usize, rate: ChatRateLimit) -> Result<Self, ChatSendError> {
        if capacity == 0 || capacity > MAX_PENDING_CHAT_SENDS {
            return Err(ChatSendError::InvalidCapacity);
        }
        Ok(Self {
            capacity,
            rate,
            session: None,
            next_sequence: 0,
            accepted_millis: VecDeque::with_capacity(rate.maximum_messages),
            pending: VecDeque::with_capacity(capacity),
            last_millis: None,
        })
    }

    pub fn pending(&self) -> &VecDeque<ChatSendRequest> {
        &self.pending
    }

    pub fn begin_session(&mut self, session: u64) -> usize {
        if self.session == Some(session) {
            return 0;
        }
        let dropped = self.pending.len();
        self.pending.clear();
        self.accepted_millis.clear();
        self.session = Some(session);
        self.next_sequence = 0;
        self.last_millis = None;
        dropped
    }

    pub fn push(
        &mut self,
        session: u64,
        message: &str,
        now_millis: u64,
    ) -> Result<ChatSendRequest, ChatSendError> {
        if message.is_empty() {
            return Err(ChatSendError::EmptyMessage);
        }
        if message.len() > MAX_CHAT_INPUT_BYTES {
            return Err(ChatSendError::MessageTooLong);
        }
        match self.session {
            None => {
                self.begin_session(session);
            }
            Some(expected) if expected != session => {
                return Err(ChatSendError::WrongSession {
                    expected,
                    actual: session,
                });
            }
            Some(_) => {}
        }
        if let Some(previous) = self.last_millis
            && now_millis < previous
        {
            return Err(ChatSendError::NonMonotonicClock {
                previous,
                actual: now_millis,
            });
        }
        if self.pending.len() >= self.capacity {
            return Err(ChatSendError::QueueFull);
        }
        while self
            .accepted_millis
            .front()
            .is_some_and(|accepted| now_millis.saturating_sub(*accepted) >= self.rate.window_millis)
        {
            self.accepted_millis.pop_front();
        }
        if self.accepted_millis.len() >= self.rate.maximum_messages {
            return Err(ChatSendError::RateLimited);
        }
        let request = ChatSendRequest {
            session,
            sequence: self.next_sequence,
            message: Arc::from(message),
        };
        self.next_sequence = self.next_sequence.saturating_add(1);
        self.last_millis = Some(now_millis);
        self.accepted_millis.push_back(now_millis);
        self.pending.push_back(request.clone());
        Ok(request)
    }

    pub fn confirm_front(&mut self, sequence: u64) -> bool {
        if self.pending.front().map(|request| request.sequence) != Some(sequence) {
            return false;
        }
        self.pending.pop_front();
        true
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChatAutocompleteAction {
    Add,
    Remove,
    Replace,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChatAutocompleteDelta {
    pub enum_name: Arc<str>,
    pub action: ChatAutocompleteAction,
    pub suggestions: Arc<[Arc<str>]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChatAutocompleteRequest {
    pub session: u64,
    pub input_revision: u64,
    pub request_id: u64,
    pub cursor_byte: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChatAutocompleteApply {
    Applied,
    IgnoredStaleInput,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChatAutocompleteError {
    InputTooLong,
    InvalidCursor,
    TooManySuggestions,
    SuggestionsTooLarge,
}

#[derive(Clone, Debug, Default)]
pub struct ChatAutocompleteState {
    next_request_id: u64,
    active: Option<ChatAutocompleteRequest>,
    suggestions: Vec<Arc<str>>,
    retained_bytes: usize,
}

impl ChatAutocompleteState {
    pub fn begin_input(
        &mut self,
        session: u64,
        input_revision: u64,
        input: &str,
        cursor_byte: usize,
    ) -> Result<ChatAutocompleteRequest, ChatAutocompleteError> {
        if input.len() > MAX_CHAT_INPUT_BYTES {
            return Err(ChatAutocompleteError::InputTooLong);
        }
        if cursor_byte > input.len() || !input.is_char_boundary(cursor_byte) {
            return Err(ChatAutocompleteError::InvalidCursor);
        }
        let request = ChatAutocompleteRequest {
            session,
            input_revision,
            request_id: self.next_request_id,
            cursor_byte: u16::try_from(cursor_byte)
                .expect("chat input maximum is bounded below u16::MAX"),
        };
        self.next_request_id = self.next_request_id.saturating_add(1);
        self.active = Some(request);
        self.clear_suggestions();
        Ok(request)
    }

    pub fn apply(
        &mut self,
        request: ChatAutocompleteRequest,
        delta: ChatAutocompleteDelta,
    ) -> Result<ChatAutocompleteApply, ChatAutocompleteError> {
        if self.active != Some(request) {
            return Ok(ChatAutocompleteApply::IgnoredStaleInput);
        }
        if delta.suggestions.len() > MAX_CHAT_AUTOCOMPLETE {
            return Err(ChatAutocompleteError::TooManySuggestions);
        }
        let delta_bytes = delta
            .suggestions
            .iter()
            .map(|value| value.len())
            .sum::<usize>();
        if delta_bytes > MAX_CHAT_AUTOCOMPLETE_BYTES {
            return Err(ChatAutocompleteError::SuggestionsTooLarge);
        }
        let mut next = match delta.action {
            ChatAutocompleteAction::Replace => Vec::new(),
            ChatAutocompleteAction::Add | ChatAutocompleteAction::Remove => {
                self.suggestions.clone()
            }
        };
        match delta.action {
            ChatAutocompleteAction::Add | ChatAutocompleteAction::Replace => {
                for suggestion in delta.suggestions.iter() {
                    if !next.contains(suggestion) {
                        next.push(Arc::clone(suggestion));
                    }
                }
            }
            ChatAutocompleteAction::Remove => {
                next.retain(|current| !delta.suggestions.contains(current));
            }
        }
        let retained_bytes = next.iter().map(|value| value.len()).sum::<usize>();
        if next.len() > MAX_CHAT_AUTOCOMPLETE {
            return Err(ChatAutocompleteError::TooManySuggestions);
        }
        if retained_bytes > MAX_CHAT_AUTOCOMPLETE_BYTES {
            return Err(ChatAutocompleteError::SuggestionsTooLarge);
        }
        self.suggestions = next;
        self.retained_bytes = retained_bytes;
        Ok(ChatAutocompleteApply::Applied)
    }

    pub fn suggestions(&self) -> &[Arc<str>] {
        &self.suggestions
    }

    pub const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    pub fn clear(&mut self) {
        self.active = None;
        self.clear_suggestions();
    }

    fn clear_suggestions(&mut self) {
        self.suggestions.clear();
        self.retained_bytes = 0;
    }
}

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
