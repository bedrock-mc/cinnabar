/// Generates typed asset constants and refuses a missing or renamed asset.
fn main() {
    experience_sdk::declarations::generate("experience.toml");
}
