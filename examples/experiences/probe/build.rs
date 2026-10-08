//! Writes the declarations of the probe's `experience.toml` for `include_declarations!`.

fn main() {
    experience_sdk::declarations::generate("experience.toml");
}
