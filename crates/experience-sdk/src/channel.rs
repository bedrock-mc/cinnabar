//! Typed channels between the two halves, as `experience.toml` declares them.

/// The half that sends on a channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// The server half sends; the client part receives the record in `dispatch`.
    ToClient,
    /// The client part sends; the server half receives the record in `client-message`.
    ToServer,
}

/// The type of one field of a channel record, which bounds the values it admits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Bool,
    /// An integer in `min..=max`.
    Integer {
        min: i64,
        max: i64,
    },
    /// Text of at most `max_bytes` UTF-8 bytes.
    Text {
        max_bytes: u16,
    },
    /// A zero-based choice below `variants`.
    Choice {
        variants: u16,
    },
    /// At most `max_items` values of the one type `item`.
    List {
        item: &'static Field,
        max_items: u16,
    },
    /// One value per field, in order.
    Record {
        fields: &'static [Field],
    },
}

/// A typed channel, as `experience.toml` declares it in `[[client.channels]]`. The host validates
/// every record against the declaration that the client part's `.cxb` signs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Channel {
    /// `<experience id>.<name>`.
    pub id: &'static str,
    /// The record's revision.
    pub schema: u16,
    pub direction: Direction,
    /// The record's fields, in order.
    pub fields: &'static [Field],
}

impl Channel {
    /// Whether a message names this channel and revision.
    pub fn is(&self, channel: &str, schema: u16) -> bool {
        self.id == channel && self.schema == schema
    }
}
