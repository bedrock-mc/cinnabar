//! Trusted session controller; remote data never supplies consent controls.

mod driver;
pub(crate) mod input;
mod live;
mod media;
mod worker;

pub(crate) use client_ui::experience_session::{ExperienceSession, unix_seconds};

pub(crate) use driver::configure;
