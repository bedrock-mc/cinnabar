#![expect(
    clippy::print_stderr,
    reason = "fixture helpers explain skipped tests on stderr and dump layouts for local diagnosis"
)]

mod anim;
mod animation_safety;
mod bind;
mod bind_cache;
mod bind_native;
mod bind_parity;
mod component_instances;
mod declared_pack_paths;
mod factory_lifecycle;
mod fixtures;
mod focus;
mod forms;
mod hud;
mod incremental_bind;
mod incremental_layout;
mod input;
mod interaction;
mod layout;
mod layout_parity;
mod normalized_position;
mod pack_inheritance;
mod paint_transform;
mod partial_documents;
mod render;
mod resolution;
mod resolve_snapshot;
mod screens;
mod selection_wheel;
mod server_pack;
mod support;
mod tooltip;
