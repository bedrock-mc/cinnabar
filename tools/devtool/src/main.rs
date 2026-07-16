fn main() {
    let result =
        devtool::parse_args(std::env::args().skip(1)).and_then(|options| devtool::run(&options));
    if let Err(error) = result {
        eprintln!("devtool: {error}");
        std::process::exit(1);
    }
}
