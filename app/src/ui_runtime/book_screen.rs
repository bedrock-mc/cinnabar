//! The writable-book, written-book and lectern reading screens.

use bevy::prelude::KeyCode;
use protocol::{BookEdit, MAX_BOOK_PAGES, Packet};

use super::UiRuntime;

/// Characters a writable page holds.
const MAX_PAGE_CHARS: usize = 256;
const MAX_TITLE_CHARS: usize = 16;
/// Client packets held for the network flush; extras are dropped.
pub(super) const MAX_QUEUED_CLIENT_PACKETS: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BookSource {
    /// A book in this player inventory slot.
    Held(u8),
    /// The book on the lectern at this position.
    Lectern([i32; 3]),
}

#[derive(Debug, Clone)]
pub(crate) struct BookState {
    pub(crate) source: BookSource,
    pub(crate) pages: Vec<String>,
    baseline: Vec<String>,
    pub(crate) page: usize,
    pub(crate) editable: bool,
    /// The title prompt shown before a book is signed.
    pub(crate) signing: bool,
    pub(crate) title: String,
    pub(crate) author: String,
    /// The page whose edit controls the vanilla book screen shows.
    pub(crate) editing: Option<usize>,
}

impl BookState {
    pub(crate) fn new(
        source: BookSource,
        mut pages: Vec<String>,
        editable: bool,
        title: String,
        author: String,
    ) -> Self {
        if pages.is_empty() {
            pages.push(String::new());
        }
        Self {
            source,
            baseline: pages.clone(),
            pages,
            page: 0,
            editable,
            signing: false,
            title,
            author,
            editing: None,
        }
    }

    /// Types into the current page, or the title while signing.
    pub(crate) fn type_text(&mut self, text: &str) {
        if !self.editable {
            return;
        }
        let (field, limit, newlines) = if self.signing {
            (&mut self.title, MAX_TITLE_CHARS, false)
        } else {
            (&mut self.pages[self.page], MAX_PAGE_CHARS, true)
        };
        for ch in text.chars() {
            let allowed = !ch.is_control() || (newlines && ch == '\n');
            if allowed && field.chars().count() < limit {
                field.push(ch);
            }
        }
    }

    pub(crate) fn backspace(&mut self) {
        if !self.editable {
            return;
        }
        if self.signing {
            self.title.pop();
        } else {
            self.pages[self.page].pop();
        }
    }

    /// Moves `delta` pages, staying inside the book; whether the page changed.
    pub(crate) fn turn(&mut self, delta: isize) -> bool {
        let target = self
            .page
            .saturating_add_signed(delta)
            .min(self.pages.len() - 1);
        let changed = target != self.page;
        self.page = target;
        changed
    }

    /// Appends a blank page and shows it, when the book is writable and has room.
    pub(crate) fn add_page(&mut self) -> bool {
        if !self.editable || self.signing || self.pages.len() >= MAX_BOOK_PAGES {
            return false;
        }
        self.pages.push(String::new());
        self.page = self.pages.len() - 1;
        true
    }

    /// The first page of the two-page spread showing the current page.
    pub(crate) fn spread(&self) -> usize {
        self.page - self.page % 2
    }

    /// Shows the next spread, starting a page there in a writable book; whether it moved.
    pub(crate) fn next_spread(&mut self) -> bool {
        let target = self.spread() + 2;
        if target < self.pages.len() {
            self.page = target;
            return true;
        }
        self.add_page() && {
            self.page = target.min(self.pages.len() - 1);
            true
        }
    }

    /// Shows the previous spread; whether it moved.
    pub(crate) fn prev_spread(&mut self) -> bool {
        let spread = self.spread();
        self.page = spread.saturating_sub(2);
        spread > 0
    }

    /// Inserts a blank page after `at` and types into it.
    pub(crate) fn insert_page(&mut self, at: usize) {
        if !self.editable || self.pages.len() >= MAX_BOOK_PAGES || at >= self.pages.len() {
            return;
        }
        self.pages.insert(at + 1, String::new());
        self.page = at + 1;
    }

    /// Removes page `at`; the last page left only clears.
    pub(crate) fn delete_page(&mut self, at: usize) {
        if !self.editable || at >= self.pages.len() {
            return;
        }
        if self.pages.len() == 1 {
            self.pages[0].clear();
            return;
        }
        self.pages.remove(at);
        self.page = self.page.min(self.pages.len() - 1);
    }

    /// Swaps page `at` with page `with`, both inside the book.
    pub(crate) fn swap_pages(&mut self, at: usize, with: usize) {
        if self.editable && at < self.pages.len() && with < self.pages.len() {
            self.pages.swap(at, with);
        }
    }

    /// The page edits that bring the server's copy in line, oldest page first.
    pub(crate) fn edits(&self) -> Vec<BookEdit> {
        self.pages
            .iter()
            .enumerate()
            .filter_map(|(index, text)| {
                let page = i32::try_from(index).ok()?;
                match self.baseline.get(index) {
                    Some(old) if old == text => None,
                    Some(_) => Some(BookEdit::ReplacePage {
                        page,
                        text: text.clone(),
                    }),
                    None => Some(BookEdit::AddPage {
                        page,
                        text: text.clone(),
                    }),
                }
            })
            // Pages deleted past the new end go last page first.
            .chain(
                (self.pages.len()..self.baseline.len())
                    .rev()
                    .filter_map(|page| {
                        Some(BookEdit::DeletePage {
                            page: i32::try_from(page).ok()?,
                        })
                    }),
            )
            .collect()
    }
}

impl UiRuntime {
    /// Queues a client packet for the next network flush.
    pub(crate) fn queue_client_packet(&mut self, packet: Packet) {
        if self.client_packets.len() < MAX_QUEUED_CLIENT_PACKETS {
            self.client_packets.push_back(packet);
        }
    }

    pub(crate) fn take_client_packet(&mut self) -> Option<Packet> {
        self.client_packets
            .pop_front()
            .or_else(|| self.book_packets.pop_front())
    }

    pub(crate) fn requeue_client_packet(&mut self, packet: Packet) {
        self.client_packets.push_front(packet);
    }

    /// Shows a book screen over the world.
    pub(crate) fn open_book(&mut self, state: BookState) {
        self.screen.book = Some(state);
        self.inventory_open = true;
        self.chat_focused = false;
    }

    /// Opens the book in the selected hotbar slot; whether one was there.
    pub(crate) fn open_held_book(
        &mut self,
        player_runtime: &crate::player_runtime::PlayerRuntime,
    ) -> bool {
        let Some(slot) = self.selected_hotbar_slot(player_runtime) else {
            return false;
        };
        let Some(stack) = player_runtime.inventory.ledger().displayed_stack(slot) else {
            return false;
        };
        let Some(entry) = player_runtime
            .inventory
            .ledger()
            .negotiated_item_entry(stack.network_id)
        else {
            return false;
        };
        let editable = match entry.identifier.as_ref() {
            "minecraft:writable_book" => true,
            "minecraft:written_book" => false,
            _ => return false,
        };
        let book = protocol::item_book(&stack.extra_data);
        let state = BookState::new(
            BookSource::Held(slot),
            book.pages.iter().map(ToString::to_string).collect(),
            editable,
            book.title.as_deref().unwrap_or_default().to_owned(),
            book.author.as_deref().unwrap_or_default().to_owned(),
        );
        self.open_book(state);
        true
    }

    /// Retains a complete book commit before removing the editable screen state.
    pub(crate) fn commit_book(&mut self, sign: bool) -> bool {
        let Some(book) = self.screen.book.as_ref() else {
            return true;
        };
        let BookSource::Held(slot) = book.source else {
            self.screen.book = None;
            return true;
        };
        if !book.editable {
            self.screen.book = None;
            return true;
        }
        if !self.book_packets.is_empty()
            || book.pages.len() > MAX_BOOK_PAGES
            || book.baseline.len() > MAX_BOOK_PAGES
        {
            return false;
        }
        let mut packets: std::collections::VecDeque<_> = book
            .edits()
            .iter()
            .filter_map(|edit| protocol::book_edit_packet(slot, edit))
            .collect();
        if sign
            && let Some(packet) = protocol::book_edit_packet(
                slot,
                &BookEdit::Finalize {
                    title: book.title.clone(),
                    author: book.author.clone(),
                    xuid: String::new(),
                },
            )
        {
            packets.push_back(packet);
        }
        self.book_packets = packets;
        self.screen.book = None;
        true
    }

    /// Closes the book after all its edits have been retained for transport.
    pub(crate) fn finish_book(
        &mut self,
        player_runtime: &mut crate::player_runtime::PlayerRuntime,
        sign: bool,
    ) {
        if self.commit_book(sign) {
            self.close_inventory(player_runtime);
        }
    }

    /// Book navigation keys: arrows and page keys turn pages; Enter starts a
    /// new line, or signs the book from the title prompt. Whether the key was used.
    pub(crate) fn book_key(
        &mut self,
        player_runtime: &mut crate::player_runtime::PlayerRuntime,
        key: KeyCode,
    ) -> bool {
        let Some(book) = self.screen.book.as_mut() else {
            return false;
        };
        match key {
            KeyCode::Enter | KeyCode::NumpadEnter if book.editable => {
                if book.signing {
                    self.finish_book(player_runtime, true);
                } else {
                    book.type_text("\n");
                }
                true
            }
            KeyCode::ArrowLeft | KeyCode::PageUp | KeyCode::ArrowRight | KeyCode::PageDown => {
                let forward = matches!(key, KeyCode::ArrowRight | KeyCode::PageDown);
                if book.turn(if forward { 1 } else { -1 }) {
                    self.report_lectern_page();
                }
                true
            }
            _ => false,
        }
    }

    /// Reports a lectern page turn to the server.
    pub(crate) fn report_lectern_page(&mut self) {
        let Some(book) = self.screen.book.as_ref() else {
            return;
        };
        let BookSource::Lectern(position) = book.source else {
            return;
        };
        let packet = protocol::lectern_update_packet(
            u8::try_from(book.page).unwrap_or(u8::MAX),
            u8::try_from(book.pages.len()).unwrap_or(u8::MAX),
            position,
        );
        self.queue_client_packet(packet);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book(editable: bool) -> BookState {
        BookState::new(
            BookSource::Held(0),
            vec!["one".to_owned()],
            editable,
            String::new(),
            String::new(),
        )
    }

    // Spreads turn two pages; deleting past the old end reports the deletions.
    #[test]
    fn spreads_turn_by_two_and_deletions_reach_the_edits() {
        let mut state = BookState::new(
            BookSource::Held(0),
            ["a", "b", "c"].map(str::to_owned).to_vec(),
            true,
            String::new(),
            String::new(),
        );
        assert!(state.next_spread());
        assert_eq!((state.page, state.spread()), (2, 2));
        // At the end a writable book starts a page, here the spread's right one.
        assert!(state.next_spread());
        assert_eq!((state.pages.len(), state.page), (4, 3));
        assert!(state.prev_spread());
        assert_eq!(state.spread(), 0);
        state.delete_page(3);
        state.delete_page(0);
        state.swap_pages(0, 1);
        assert_eq!(state.pages, ["c", "b"]);
        let edits = state.edits();
        assert!(
            edits.contains(&BookEdit::DeletePage { page: 2 }),
            "{edits:?}"
        );
    }

    #[test]
    fn editing_tracks_replaced_and_added_pages() {
        let mut state = book(true);
        state.type_text("!");
        assert!(state.add_page());
        state.type_text("two");
        let edits = state.edits();
        assert_eq!(
            edits,
            vec![
                BookEdit::ReplacePage {
                    page: 0,
                    text: "one!".into()
                },
                BookEdit::AddPage {
                    page: 1,
                    text: "two".into()
                },
            ]
        );
    }

    #[test]
    fn read_only_books_ignore_typing_and_new_pages() {
        let mut state = book(false);
        state.type_text("x");
        assert!(!state.add_page());
        assert!(state.edits().is_empty());
        assert!(!state.turn(1));
    }

    #[test]
    fn page_text_is_bounded() {
        let mut state = book(true);
        state.type_text(&"a".repeat(400));
        assert_eq!(state.pages[0].chars().count(), MAX_PAGE_CHARS);
        state.signing = true;
        state.type_text(&"t".repeat(40));
        assert_eq!(state.title.chars().count(), MAX_TITLE_CHARS);
    }
}

#[cfg(test)]
#[path = "book_screen_tests.rs"]
mod regression_tests;
