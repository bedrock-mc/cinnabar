//! Local crash report folders: schema, layout, log sections and retention. Nothing is uploaded.
//!
//! Each crash owns `crashes/<UTC yyyymmdd-hhmmss>-<id>/` holding `report.json` and the crashing
//! session's sections of `client.log` and `core.log`. Callers supply the time; this crate never
//! reads a clock.

pub mod folder;
pub mod native;
#[cfg(unix)]
pub mod prepared;
pub mod report;
pub mod section;
mod utc;
