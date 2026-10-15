//! Validated screen mutations and bounded rollback of one changed data entry.

use crate::ModScreens;
use server_experience::screen::{Modal, Row, Value};
use std::{collections::BTreeMap, io::Write};

/// One callback's screen copy, published only if at least one write was accepted.
pub(in crate::runtime) struct ScreenTransaction {
    screens: ModScreens,
    changed: bool,
}

impl ScreenTransaction {
    /// Copies retained output once, keeping the committed screens isolated from all writes.
    pub(super) fn new(screens: &ModScreens) -> Self {
        Self {
            screens: screens.clone(),
            changed: false,
        }
    }

    /// Reuses this copy after accepted or rejected writes and remembers accepted output.
    pub(super) fn apply(&mut self, write: ScreenWrite) -> Result<(), String> {
        write.apply(&mut self.screens)?;
        self.changed = true;
        Ok(())
    }

    /// Returns accepted output without replacing committed screens for a denied-only callback.
    pub(in crate::runtime) fn commit(self) -> Option<ModScreens> {
        self.changed.then_some(self.screens)
    }

    /// Exposes private staging for deterministic regression witnesses.
    #[cfg(test)]
    pub(super) fn screens(&self) -> &ModScreens {
        &self.screens
    }
}

/// A write whose names, declarations and payload have already been validated.
pub(super) enum ScreenWrite {
    Overlay(Option<String>),
    View(Option<String>),
    Collection(String, Vec<Row>),
    Value(String, Value),
    Text(String, String),
    Focus(String),
}

impl ScreenWrite {
    /// Applies a write, restoring its previous entry on aggregate-bound failure.
    fn apply(self, screens: &mut ModScreens) -> Result<(), String> {
        screens.data.check().map_err(|error| error.to_string())?;
        let revision = screens.data.revision + 1;
        match self {
            Self::Overlay(template) => screens.overlay = template,
            Self::View(template) => screens.view = template,
            Self::Collection(name, rows) => {
                replace(&mut screens.data, name, rows.into(), |data| {
                    &mut data.collections
                })?;
            }
            Self::Value(name, value) => {
                replace(&mut screens.data, name, value, |data| &mut data.values)?;
            }
            Self::Text(control, text) => {
                replace(&mut screens.data, control, (revision, text), |data| {
                    &mut data.texts
                })?;
            }
            Self::Focus(control) => screens.focus = Some((revision, control)),
        }
        screens.data.revision = revision;
        Ok(())
    }
}

/// Replaces one entry and restores it if the modal would exceed its retained-data bounds.
fn replace<T>(
    data: &mut Modal,
    name: String,
    value: T,
    field: impl Fn(&mut Modal) -> &mut BTreeMap<String, T>,
) -> Result<(), String> {
    let previous = field(data).insert(name.clone(), value);
    if let Err(error) = data.check() {
        match previous {
            Some(previous) => {
                field(data).insert(name, previous);
            }
            None => {
                field(data).remove(&name);
            }
        }
        return Err(error.to_string());
    }
    Ok(())
}

/// Counts serialized bytes with the same JSON encoding as the output budget, without a buffer.
pub(super) fn value_bytes(value: &Value) -> usize {
    let mut counter = ByteCount(0);
    serde_json::to_writer(&mut counter, value).map_or(0, |()| counter.0)
}

/// A JSON output sink that retains only its byte count.
struct ByteCount(usize);

impl Write for ByteCount {
    /// Counts every byte accepted by the serializer.
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 += bytes.len();
        Ok(bytes.len())
    }

    /// Completes a write without retaining any buffered output.
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
