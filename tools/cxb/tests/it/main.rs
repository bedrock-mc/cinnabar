mod cli;
mod contracts;
mod cosmic;
mod fixtures;
mod media;
mod offline;
mod project;

/// Supplies a native drawing fence for in-process client-part conformance tests.
fn drawing_context(epoch: u64) -> mod_host::presentation::Context {
    mod_host::presentation::Context {
        connection: 1,
        epoch,
        ..Default::default()
    }
}
