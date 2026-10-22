//! Links the Go core's C archive into the iOS app, which runs the core in-process; other targets
//! build an empty program.

use std::{env, path::PathBuf};

const ARCHIVE_ENV: &str = "CINNABAR_IOS_CORE_ARCHIVE";

fn main() {
    println!("cargo:rerun-if-env-changed={ARCHIVE_ENV}");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("ios") {
        return;
    }
    let Some(archive) = env::var_os(ARCHIVE_ENV).map(PathBuf::from) else {
        panic!(
            "{ARCHIVE_ENV} must name the Go core's libbedrock_core.a; packaging/ios/build-ipa.sh builds it"
        );
    };
    let (Some(directory), Some(name)) = (
        archive.parent(),
        archive
            .file_stem()
            .and_then(|stem| stem.to_str())
            .and_then(|stem| stem.strip_prefix("lib")),
    ) else {
        panic!(
            "{ARCHIVE_ENV} must name a lib<name>.a archive: {}",
            archive.display()
        );
    };
    println!("cargo:rerun-if-changed={}", archive.display());
    println!("cargo:rustc-link-search=native={}", directory.display());
    println!("cargo:rustc-link-lib=static={name}");
    // The Go runtime's TLS roots and DNS resolver.
    println!("cargo:rustc-link-lib=framework=CoreFoundation");
    println!("cargo:rustc-link-lib=framework=Security");
    println!("cargo:rustc-link-lib=resolv");
}
