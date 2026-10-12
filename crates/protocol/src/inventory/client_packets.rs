//! Client packets tied to inventory screens: block pick and book editing.

use bedrock_protocol_1_26_51::packets::{
    BlockPickRequest, BookEdit as BookEditPacket, LecternUpdate, PlayerToggleCrafterSlotRequest,
};
use bedrock_protocol_1_26_51::types::{BlockPos, BookEditAction};
use bedrock_protocol_1_26_51::wire;

/// The wire block position of `position`.
fn block_pos(position: [i32; 3]) -> BlockPos {
    BlockPos {
        x: wire::ZigZag32(position[0]),
        y: wire::ZigZag32(position[1]),
        z: wire::ZigZag32(position[2]),
    }
}

/// Longest string a book edit carries: the codec's bound on every book-edit text, which a test
/// pins to the generated encoder.
pub const MAX_BOOK_PAGE_BYTES: usize = 768;

/// Asks the server to put the block at `position` in the player's hand.
#[must_use]
pub fn block_pick_request_packet(position: [i32; 3], with_data: bool) -> crate::Packet {
    crate::Packet::new(BlockPickRequest {
        position: block_pos(position),
        with_data,
        max_slots: wire::U8(9),
    })
}

/// Reports a lectern's page turn to the server.
#[must_use]
pub fn lectern_update_packet(page: u8, total_pages: u8, position: [i32; 3]) -> crate::Packet {
    crate::Packet::new(LecternUpdate {
        new_page_to_show: wire::U8(page),
        total_pages: wire::U8(total_pages),
        position_of_lectern_to_update: block_pos(position),
    })
}

/// Asks the server to disable or enable one slot of the crafter at `position`.
#[must_use]
pub fn crafter_slot_toggle_packet(position: [i32; 3], slot: u8, disabled: bool) -> crate::Packet {
    crate::Packet::new(PlayerToggleCrafterSlotRequest {
        pos_x: wire::I32LE(position[0]),
        pos_y: wire::I32LE(position[1]),
        pos_z: wire::I32LE(position[2]),
        slot_index: wire::U8(slot),
        is_disabled: disabled,
    })
}

/// One edit of a writable book.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum BookEdit {
    ReplacePage {
        page: i32,
        text: String,
    },
    AddPage {
        page: i32,
        text: String,
    },
    DeletePage {
        page: i32,
    },
    SwapPages {
        page: i32,
        with: i32,
    },
    /// Signs the book.
    Finalize {
        title: String,
        author: String,
        xuid: String,
    },
}

/// Builds the edit for the book in inventory slot `book_slot`; `None` when a
/// page index is negative or any text exceeds [`MAX_BOOK_PAGE_BYTES`].
#[must_use]
pub fn book_edit_packet(book_slot: u8, edit: &BookEdit) -> Option<crate::Packet> {
    let operation = match edit {
        BookEdit::ReplacePage { page, text } | BookEdit::AddPage { page, text } => {
            if *page < 0 || text.len() > MAX_BOOK_PAGE_BYTES {
                return None;
            }
            let page_index = wire::ZigZag32(*page);
            let page_text = text.as_str().into();
            let photo_name = wire::Str::default();
            if matches!(edit, BookEdit::ReplacePage { .. }) {
                BookEditAction::ReplacePage {
                    page_index,
                    page_text,
                    photo_name,
                }
            } else {
                BookEditAction::AddPage {
                    page_index,
                    page_text,
                    photo_name,
                }
            }
        }
        BookEdit::DeletePage { page } if *page >= 0 => BookEditAction::DeletePage {
            page_index: wire::ZigZag32(*page),
        },
        BookEdit::SwapPages { page, with } if *page >= 0 && *with >= 0 => {
            BookEditAction::SwapPages {
                page_index: wire::ZigZag32(*page),
                swap_with_index: wire::ZigZag32(*with),
            }
        }
        BookEdit::Finalize {
            title,
            author,
            xuid,
        } if [title, author, xuid]
            .iter()
            .all(|text| text.len() <= MAX_BOOK_PAGE_BYTES) =>
        {
            BookEditAction::Finalize {
                title: title.as_str().into(),
                author: author.as_str().into(),
                xuid: xuid.as_str().into(),
            }
        }
        _ => return None,
    };
    Some(crate::Packet::new(BookEditPacket {
        book_slot: wire::ZigZag32(i32::from(book_slot)),
        operation,
    }))
}

#[cfg(test)]
mod tests {
    use bedrock_protocol_1_26_51::packets;

    use super::*;

    #[test]
    fn book_edits_validate_pages() {
        assert!(book_edit_packet(0, &BookEdit::DeletePage { page: -1 }).is_none());
        let packet = book_edit_packet(
            3,
            &BookEdit::ReplacePage {
                page: 1,
                text: "hi".into(),
            },
        )
        .unwrap();
        let packets::Packet::BookEdit(edit) = *packet.data else {
            panic!("a book edit");
        };
        assert_eq!(edit.book_slot.0, 3);
    }

    /// The page limit is exactly the bound the generated encoder enforces.
    #[test]
    fn book_text_limit_matches_the_codec_bound() {
        use bedrock_protocol_1_26_51::wire::Encode;
        let page = |bytes: usize| BookEditAction::ReplacePage {
            page_index: wire::ZigZag32(0),
            page_text: "a".repeat(bytes).into(),
            photo_name: wire::Str::default(),
        };
        assert!(page(MAX_BOOK_PAGE_BYTES).encode_to_vec().is_ok());
        assert!(matches!(
            page(MAX_BOOK_PAGE_BYTES + 1).encode_to_vec(),
            Err(wire::EncodeError::SchemaConstraint(_))
        ));
    }

    #[test]
    fn over_limit_book_text_is_refused_before_encoding() {
        let long = "a".repeat(MAX_BOOK_PAGE_BYTES + 1);
        assert!(
            book_edit_packet(
                0,
                &BookEdit::AddPage {
                    page: 0,
                    text: long.clone(),
                },
            )
            .is_none()
        );
        assert!(
            book_edit_packet(
                0,
                &BookEdit::Finalize {
                    title: "t".into(),
                    author: long,
                    xuid: String::new(),
                },
            )
            .is_none()
        );
        let at_limit = BookEdit::ReplacePage {
            page: 0,
            text: "a".repeat(MAX_BOOK_PAGE_BYTES),
        };
        assert!(crate::encode(&book_edit_packet(0, &at_limit).unwrap()).is_ok());
    }

    #[test]
    fn block_pick_carries_the_position() {
        let packets::Packet::BlockPickRequest(pick) =
            *block_pick_request_packet([1, 2, 3], false).data
        else {
            panic!("a block pick");
        };
        assert_eq!(
            (pick.position.x.0, pick.position.y.0, pick.position.z.0),
            (1, 2, 3)
        );
    }

    #[test]
    fn crafter_toggle_names_the_block_and_slot() {
        let packets::Packet::PlayerToggleCrafterSlotRequest(toggle) =
            *crafter_slot_toggle_packet([4, 5, 6], 7, true).data
        else {
            panic!("a crafter toggle");
        };
        assert_eq!((toggle.pos_x.0, toggle.pos_y.0, toggle.pos_z.0), (4, 5, 6));
        assert_eq!((toggle.slot_index.0, toggle.is_disabled), (7, true));
    }
}
