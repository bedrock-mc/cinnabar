#![expect(
    clippy::print_stderr,
    reason = "devtool reports command failures on stderr"
)]

fn main() {
    if let Err(error) = devtool::dispatch(std::env::args().skip(1).collect()) {
        eprintln!("devtool: {error}");
        std::process::exit(1);
    }
}
