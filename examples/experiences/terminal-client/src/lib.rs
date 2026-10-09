//! Test client part shaped like SP3's ME Terminal, for the host's fuel and failure tests.
//! `dispatch` decodes the record through the SDK, as a real client part does, and by channel:
//!
//! - `terminal.panic` panics, which traps;
//! - `terminal.spin` loops until its fuel runs out;
//! - `terminal.count` counts its calls in guest memory and binds the count as the `count`
//!   collection's one row, so a fresh instance shows up as a count of 1;
//! - `terminal.size` binds what `ui.modal-size` reads as `#read`;
//! - `modal-resized` binds its size as `#size` and reads it back as `#read`, and `text-changed`
//!   in `terminal.search` sets `terminal.echo` to the text in capitals, and `secondary-action`
//!   binds its action and row as `#secondary`, and `scroll-changed` binds its view as
//!   `#scrolled` and its range as `#scroll`;
//! - anything else binds its item list (records of an id, a count and a display name) into the
//!   `items` collection, one row per item, and binds nothing for any other record.

use std::sync::atomic::{AtomicI64, Ordering};

use experience_sdk::Value;
use experience_sdk::client::{ClientPart, ui};
use serde_json::{Map, json};

/// Calls of `terminal.count` since this instance started.
static CALLS: AtomicI64 = AtomicI64::new(0);

struct Terminal;

impl ClientPart for Terminal {
    fn init() {}

    fn dispatch(channel: String, record: Vec<Value>) {
        match channel.as_str() {
            "terminal.panic" => panic!("terminal fault"),
            "terminal.spin" => loop {
                std::hint::black_box(&channel);
            },
            "terminal.size" => read_size(),
            "terminal.count" => {
                let calls = CALLS.fetch_add(1, Ordering::Relaxed) + 1;
                let mut row = Map::new();
                row.insert("#count".into(), json!({"type": "integer", "value": calls}));
                bind("count", &[row]);
            }
            _ => items(&record),
        }
    }

    /// Binds the size it was given as `#size` and what `ui.modal-size` reads as `#read`.
    fn modal_resized(size: ui::GuiSize) {
        let _ = ui::set_value(
            "#size",
            &ui::Value::Numbers(vec![size.width, size.height, size.scale]),
        );
        read_size();
    }

    /// Binds a secondary press's action and row as `#secondary`.
    fn secondary_action(id: String, collection_index: Option<u32>) {
        let row = collection_index.map_or_else(|| "none".to_owned(), |row| row.to_string());
        let _ = ui::set_value("#secondary", &ui::Value::Text(format!("{id} {row}")));
    }

    /// Binds the range a scroll view reports, with its name, as `#scroll` and `#scrolled`.
    fn scroll_changed(view: String, range: ui::ScrollRange) {
        let _ = ui::set_value("#scrolled", &ui::Value::Text(view));
        let _ = ui::set_value(
            "#scroll",
            &ui::Value::Numbers(vec![range.offset, range.viewport, range.content]),
        );
    }

    /// Answers text typed into `terminal.search` by setting `terminal.echo` to it in capitals.
    fn text_changed(control: String, text: String) {
        if control == "terminal.search" {
            let _ = ui::set_text("terminal.echo", &text.to_uppercase());
        }
    }
}

/// Binds the modal size `ui.modal-size` reads as `#read`: its width, height and scale, or
/// `false` for none.
fn read_size() {
    let read = ui::modal_size().map_or(ui::Value::Boolean(false), |size| {
        ui::Value::Numbers(vec![size.width, size.height, size.scale])
    });
    let _ = ui::set_value("#read", &read);
}

/// Binds the records of the item list in `record` as the `items` collection.
fn items(record: &[Value]) {
    let [Value::List(items)] = record else {
        return;
    };
    let rows: Vec<Map<String, serde_json::Value>> = items
        .iter()
        .filter_map(|item| match item {
            Value::Record(fields) => match fields.as_slice() {
                [Value::Text(id), Value::Integer(count), Value::Text(name)] => {
                    Some(row(id, *count, name))
                }
                _ => None,
            },
            _ => None,
        })
        .collect();
    bind("items", &rows);
}

fn bind(collection: &str, rows: &[Map<String, serde_json::Value>]) {
    let rows = serde_json::to_vec(rows).expect("rows serialize");
    let _ = ui::set_collection(collection, &rows);
}

/// One collection row in the host's `{"#binding": {"type": …, "value": …}}` form.
fn row(id: &str, count: i64, name: &str) -> Map<String, serde_json::Value> {
    let mut row = Map::new();
    row.insert("#id".into(), json!({"type": "text", "value": id}));
    row.insert("#count".into(), json!({"type": "integer", "value": count}));
    row.insert("#name".into(), json!({"type": "text", "value": name}));
    row
}

experience_sdk::export_client_part!(Terminal);
