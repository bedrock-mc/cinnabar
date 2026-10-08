use std::{ffi::OsString, sync::OnceLock};

use serde::Deserialize;

use crate::install_layout::InstallLayout;

const BEDROCK_TARGET_JSON: &str = include_str!("../../../assets/bedrock-target.json");

/// The dedicated-server build local worlds run, from the target manifest.
#[derive(Debug, Deserialize)]
struct ServerPin {
    server_version: String,
    bds_container_image: String,
}

fn server_pin() -> &'static ServerPin {
    static PIN: OnceLock<ServerPin> = OnceLock::new();
    PIN.get_or_init(|| {
        serde_json::from_str(BEDROCK_TARGET_JSON).expect("valid Bedrock target manifest")
    })
}

/// Core arguments that enable local worlds on the manifest's exact server build and image.
pub(crate) fn core_args(layout: &InstallLayout) -> Vec<OsString> {
    let pin = server_pin();
    vec![
        "-local-worlds-dir".into(),
        layout.local_worlds_dir().into(),
        "-bds-version".into(),
        pin.server_version.clone().into(),
        "-bds-image".into(),
        pin.bds_container_image.clone().into(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arg_after(args: &[OsString], flag: &str) -> String {
        let position = args.iter().position(|arg| arg == flag).expect(flag);
        args[position + 1].to_string_lossy().into_owned()
    }

    #[test]
    fn core_args_carry_the_worlds_dir_and_the_manifest_pin() {
        let layout = crate::install_layout::scratch("local-world-args");
        let args = core_args(&layout);
        assert_eq!(
            std::path::PathBuf::from(arg_after(&args, "-local-worlds-dir")),
            layout.local_worlds_dir()
        );
        assert_eq!(
            arg_after(&args, "-bds-version"),
            server_pin().server_version
        );
        assert_eq!(
            arg_after(&args, "-bds-image"),
            server_pin().bds_container_image
        );
    }

    /// A tag alone drifts with upstream releases and can break joins; the image must name a digest.
    #[test]
    fn the_container_image_is_pinned_to_a_digest() {
        let image = &server_pin().bds_container_image;
        let (name, digest) = image.split_once("@sha256:").expect("digest-pinned image");
        assert!(!name.is_empty() && !name.ends_with(":latest"), "{image}");
        assert!(
            digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit()),
            "{image}"
        );
        let version = &server_pin().server_version;
        assert_eq!(version.split('.').count(), 4, "exact build: {version}");
    }
}
