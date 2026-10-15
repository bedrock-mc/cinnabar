#![expect(
    clippy::print_stderr,
    reason = "fixture helpers explain on stderr why a test skips when a local fixture is missing"
)]

mod atmosphere;
mod audio_pcm;
mod block_icons;
mod compiler;
mod compiler_digest;
mod entity;
#[path = "support/fixture_input.rs"]
mod fixture_input;
mod hud;
mod public_font_api;
mod public_outline_font_api;
mod registry_policy;
