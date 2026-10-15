#![expect(
    clippy::print_stderr,
    reason = "dist-local reports command failures on stderr"
)]

fn main() {
    if let Err(error) = dist_local::run_from(std::env::args().skip(1)) {
        eprintln!("dist-local: {error}");
        std::process::exit(1);
    }
}
