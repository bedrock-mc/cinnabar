/// Generates both halves' shared typed definitions and registration.
fn main() {
    experience_sdk::project::generate().expect("generate terminal project");
}
