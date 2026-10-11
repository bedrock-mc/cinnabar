fn main() {
    if let Err(error) = devtool::dispatch(std::env::args().skip(1).collect()) {
        eprintln!("devtool: {error}");
        std::process::exit(1);
    }
}
