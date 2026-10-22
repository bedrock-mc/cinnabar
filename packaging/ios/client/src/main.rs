//! iOS app entry; elsewhere this program does nothing, so desktop builds still compile it.

#[cfg(target_os = "ios")]
mod ios;

fn main() {
    #[cfg(target_os = "ios")]
    ios::main();
}
