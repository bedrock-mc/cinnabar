//! Client packets tied to inventory screens: block pick and book editing.

use valentine::bedrock::version::v1_26_51::{
    BlockPickRequestPacket, BlockPos, BookEditActionAddPage, BookEditActionDeletePage,
    BookEditActionFinalize, BookEditActionReplacePage, BookEditActionSwapPages, BookEditPacket,
    BookEditPacketOperation, LecternUpdatePacket, PlayerToggleCrafterSlotRequestPacket,
};

/// Longest page text a book edit carries.
pub const MAX_BOOK_PAGE_BYTES: usize = 1_024;

/// Asks the server to put the block at `position` in the player's hand.
#[must_use]
pub fn block_pick_request_packet(position: [i32; 3], with_data: bool) -> crate::Packet {
    crate::Packet::new(BlockPickRequestPacket {
        position: BlockPos {
            x: position[0],
            y: position[1],
            z: position[2],
        },
        with_data,
        max_slots: 9,
    })
}

/// Reports a lectern's page turn to the server.
#[must_use]
pub fn lectern_update_packet(page: u8, total_pages: u8, position: [i32; 3]) -> crate::Packet {
    crate::Packet::new(LecternUpdatePacket {
        newpagetoshow: page,
        total_pages,
        positionof_lecterntoupdate: BlockPos {
            x: position[0],
            y: position[1],
            z: position[2],
        },
    })
}

/// Asks the server to disable or enable one slot of the crafter at `position`.
#[must_use]
pub fn crafter_slot_toggle_packet(position: [i32; 3], slot: u8, disabled: bool) -> crate::Packet {
    crate::Packet::new(PlayerToggleCrafterSlotRequestPacket {
        pos_x: position[0],
        pos_y: position[1],
        pos_z: position[2],
        slot_index: slot,
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
/// page index is negative or a page exceeds [`MAX_BOOK_PAGE_BYTES`].
#[must_use]
pub fn book_edit_packet(book_slot: u8, edit: &BookEdit) -> Option<crate::Packet> {
    let operation = match edit {
        BookEdit::ReplacePage { page, text } | BookEdit::AddPage { page, text } => {
            if *page < 0 || text.len() > MAX_BOOK_PAGE_BYTES {
                return None;
            }
            if matches!(edit, BookEdit::ReplacePage { .. }) {
                BookEditPacketOperation::ReplacePage(BookEditActionReplacePage {
                    page_index: *page,
                    page_text: text.clone(),
                    photo_name: String::new(),
                })
            } else {
                BookEditPacketOperation::AddPage(BookEditActionAddPage {
                    page_index: *page,
                    page_text: text.clone(),
                    photo_name: String::new(),
                })
            }
        }
        BookEdit::DeletePage { page } if *page >= 0 => {
            BookEditPacketOperation::DeletePage(BookEditActionDeletePage { page_index: *page })
        }
        BookEdit::SwapPages { page, with } if *page >= 0 && *with >= 0 => {
            BookEditPacketOperation::SwapPages(BookEditActionSwapPages {
                page_index: *page,
                swap_with_index: *with,
            })
        }
        BookEdit::Finalize {
            title,
            author,
            xuid,
        } => BookEditPacketOperation::Finalize(BookEditActionFinalize {
            title: title.clone(),
            author: author.clone(),
            xuid: xuid.clone(),
        }),
        _ => return None,
    };
    Some(crate::Packet::new(BookEditPacket {
        book_slot: i32::from(book_slot),
        operation,
    }))
}

#[cfg(test)]
mod tests {
    use valentine::bedrock::version::v1_26_51::McpePacketData;

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
        let McpePacketData::BookEditPacket(edit) = packet.data else {
            panic!("a book edit");
        };
        assert_eq!(edit.book_slot, 3);
    }

    #[test]
    fn block_pick_carries_the_position() {
        let McpePacketData::BlockPickRequestPacket(pick) =
            block_pick_request_packet([1, 2, 3], false).data
        else {
            panic!("a block pick");
        };
        assert_eq!(
            (pick.position.x, pick.position.y, pick.position.z),
            (1, 2, 3)
        );
    }

    #[test]
    fn crafter_toggle_names_the_block_and_slot() {
        let McpePacketData::PlayerToggleCrafterSlotRequestPacket(toggle) =
            crafter_slot_toggle_packet([4, 5, 6], 7, true).data
        else {
            panic!("a crafter toggle");
        };
        assert_eq!((toggle.pos_x, toggle.pos_y, toggle.pos_z), (4, 5, 6));
        assert_eq!((toggle.slot_index, toggle.is_disabled), (7, true));
    }
}
