//! Standalone entry point for compiler process regression tests and offline tools.

#[global_allocator]
static ALLOCATOR: shaderpack::worker::BudgetAllocator<std::alloc::System> =
    shaderpack::worker::BudgetAllocator(std::alloc::System);

/// Installs the compiler's limits and translates the request supplied on stdin.
fn main() {
    std::process::exit(shaderpack::worker::run_stdio());
}
