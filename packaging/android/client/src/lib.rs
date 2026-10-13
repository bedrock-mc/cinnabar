//! Android host: the NativeActivity entry and the setup activity's natives; elsewhere it only
//! builds the platform-free parts, so their tests run on desktop CI.

#[cfg(target_os = "android")]
mod android;
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
mod cancellation;
