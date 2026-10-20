//! The finite generated Go JSON type gate; semantic validation and raw normalization stay separate.

use super::{
    Entries, invalid, same_field,
    schema_generated::{Kind, Record},
};
use serde::Deserialize;
use serde_json::value::RawValue;
use std::io;

/// Checks every known occurrence without replacing its original ordered raw value.
pub(super) fn check(entries: &Entries) -> io::Result<()> {
    fields(Record::Client, entries)
}

/// Matches the generated field owner, ignoring only unknown well-formed values.
fn fields(record: Record, entries: &Entries) -> io::Result<()> {
    for (name, raw) in &entries.0 {
        if let Some(field) = record
            .fields()
            .iter()
            .find(|field| same_field(name, field.name))
        {
            value(field.kind, raw)?;
        }
    }
    Ok(())
}

/// Checks one finite kind; null is admitted without materializing Go defaults or reused values.
fn value(kind: Kind, raw: &RawValue) -> io::Result<()> {
    if raw.get() == "null" {
        return Ok(());
    }
    match kind {
        Kind::String => decode::<String>(raw).map(|_| ()),
        Kind::Integer(bits) => {
            let number = integer(raw)?;
            match bits {
                64 => Ok(()),
                32 => i32::try_from(number).map(|_| ()).map_err(|_| invalid()),
                _ => Err(invalid()),
            }
        }
        Kind::Bool | Kind::OptionalBool => decode::<bool>(raw).map(|_| ()),
        Kind::Float64 => {
            let number: f64 = decode(raw)?;
            if number.is_finite() {
                Ok(())
            } else {
                Err(invalid())
            }
        }
        Kind::Slice(record) => {
            for raw in decode::<Vec<&RawValue>>(raw)? {
                if raw.get() != "null" {
                    fields(record, &decode::<Entries>(raw)?)?;
                }
            }
            Ok(())
        }
        Kind::Strings(length) => {
            for raw in decode::<Vec<&RawValue>>(raw)?.into_iter().take(length) {
                value(Kind::String, raw)?;
            }
            Ok(())
        }
    }
}

/// Uses the existing JSON decoder for modeled values while keeping opaque values unconverted.
fn decode<'a, T: Deserialize<'a>>(raw: &'a RawValue) -> io::Result<T> {
    serde_json::from_str(raw.get()).map_err(|_| invalid())
}

/// Parses only an already JSON-framed integer token; exponent and fractional spellings fail.
pub(super) fn integer(raw: &RawValue) -> io::Result<i64> {
    raw.get().parse().map_err(|_| invalid())
}

#[cfg(test)]
mod tests;
