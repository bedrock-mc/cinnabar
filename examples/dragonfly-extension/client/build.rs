/// Generates channel declarations from the bundle's single authoring manifest.
fn main() {
    experience_sdk::declarations::generate("../experience.toml");
}
