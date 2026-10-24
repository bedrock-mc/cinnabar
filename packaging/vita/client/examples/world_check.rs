//! Interactive port integration fixture, NOT a standalone Bedrock world/server.
//! Uses the shared packed store, collision, movement, picking and meshing owners.

mod world_fixture;
#[cfg(target_os = "vita")]
mod world_probe;

#[cfg(target_os = "vita")]
fn main() {
    world_probe::run();
}

#[cfg(not(target_os = "vita"))]
fn main() {
    panic!("world-check runs on Vita; its fixture tests also run on the host");
}
