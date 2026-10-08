//! Media decoder process: one decode under a hard memory ceiling, spoken over stdio.

use server_experience::media::{ceiling, helper, worker::HELPER_COMMAND};

#[global_allocator]
static ALLOCATOR: ceiling::BoundedAllocator = ceiling::BoundedAllocator::new();

fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    anyhow::ensure!(
        args.len() == 1 && args[0] == HELPER_COMMAND,
        "usage: cinnabar-media-helper {HELPER_COMMAND}"
    );
    helper::serve(ceiling::contain_process(&ALLOCATOR)?)
}
