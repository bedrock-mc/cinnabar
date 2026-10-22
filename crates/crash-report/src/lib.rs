//! Local crash report folders: schema, layout, log sections and retention, and the payload an
//! opted-in upload sends. Nothing is uploaded without the player's consent.
//!
//! Each crash owns `crashes/<UTC yyyymmdd-hhmmss>-<id>/` holding `report.json` and the crashing
//! session's sections of `client.log` and `core.log`. A running client keeps the rings and
//! samples later reports read in its [`staging`] directory. Callers supply the time; this crate
//! never reads a clock.

pub mod folder;
pub mod native;
#[cfg(unix)]
pub mod prepared;
pub mod redact;
pub mod report;
pub mod ring;
pub mod section;
pub mod stack;
pub mod staging;
pub mod upload;
mod utc;

/// Names and ordering shared by log writers and report readers.
pub mod log_archive;
