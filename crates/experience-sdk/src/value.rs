//! Channel record values, which both halves build and read.

/// One value of a channel record, as the channel declares it: a scalar, or a list or record of
/// values. The server half passes records through the `server` world as pre-order nodes
/// (`server::nodes`, `server::values`); the client part, in the wire's JSON form, an array of
/// `{"type":…,"value":…}` objects.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(
    feature = "client",
    derive(serde::Serialize, serde::Deserialize),
    serde(
        tag = "type",
        content = "value",
        rename_all = "snake_case",
        deny_unknown_fields
    )
)]
pub enum Value {
    Bool(bool),
    Integer(i64),
    Text(String),
    Choice(u16),
    /// The items of a list field, all of its one item type.
    List(Vec<Value>),
    /// One value per field of a record field, in order.
    Record(Vec<Value>),
}
