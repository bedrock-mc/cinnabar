/// Generates shared messages, declarations and asset constants for both halves.
fn main() {
    experience_sdk::project::generate().expect("generate cosmic definitions");
}
