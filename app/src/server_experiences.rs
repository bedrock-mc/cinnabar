//! Trusted session controller; remote data never supplies consent controls.

mod driver;
mod live;
mod media;
mod worker;

pub(crate) use driver::configure;
