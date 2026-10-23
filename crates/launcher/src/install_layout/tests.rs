//! Install and development path resolution tests.

use super::{
    InstallEnvironment, InstallLayout, LayoutError, Platform, cargo_artifact_path,
    vanilla_pack_relative,
};
use std::path::{Path, PathBuf};

#[test]
fn physics_filename_follows_the_carrier_registry_for_every_layout() {
    for (platform, executable, home) in [
        (
            Platform::Windows,
            "C:/Cinnabar/cinnabar.exe",
            "C:/Users/dev",
        ),
        (Platform::Linux, "/opt/cinnabar/bin/cinnabar", "/home/dev"),
        (
            Platform::MacOs,
            "/Applications/Cinnabar.app/Contents/MacOS/cinnabar",
            "/Users/dev",
        ),
        (
            Platform::Linux,
            "/work/cinnabar/target/debug/cinnabar",
            "/home/dev",
        ),
    ] {
        let mut env = environment(executable, home);
        if platform == Platform::Windows {
            env.local_app_data = Some(PathBuf::from("C:/Users/dev/AppData/Local"));
        }
        let layout = InstallLayout::resolve(platform, &env).unwrap();
        assert_eq!(
            layout.physics_registry.file_name().unwrap(),
            assets::carriers::physics_registry_basename()
        );
        assert_eq!(
            layout.physics_registry.parent().unwrap(),
            if layout.is_installed() {
                layout.resource_root.join("assets")
            } else {
                layout.compiled_assets.clone()
            }
        );
    }
}

/// A test binary in a build dir outside the checkout still resolves the checkout.
#[test]
fn external_build_dir_tests_resolve_their_workspace() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.ancestors().nth(2).unwrap();
    let shared = PathBuf::from("/cache/build/cinnabar/debug/deps/launcher-0123");
    assert_eq!(
        cargo_artifact_path(shared.clone(), Some(manifest)),
        root.join("target/debug/deps/launcher-0123")
    );
    assert_eq!(cargo_artifact_path(shared.clone(), None), shared);
    let checkout = PathBuf::from("/work/cinnabar/target/debug/cinnabar");
    assert_eq!(
        cargo_artifact_path(checkout.clone(), Some(manifest)),
        checkout
    );
}

fn environment(executable: &str, home: &str) -> InstallEnvironment {
    InstallEnvironment {
        executable: PathBuf::from(executable),
        user_root: None,
        home: Some(PathBuf::from(home)),
        local_app_data: None,
        xdg_config_home: None,
        xdg_data_home: None,
        xdg_runtime_dir: None,
    }
}

#[test]
fn isolated_user_root_keeps_resources_and_all_user_paths_separate() {
    for (platform, executable, root) in [
        (Platform::Windows, "C:/app/cinnabar.exe", "D:/Zeno/profile"),
        (
            Platform::Linux,
            "/opt/app/bin/cinnabar",
            "/tmp/zeno-profile",
        ),
        (
            Platform::MacOs,
            "/Applications/Cinnabar.app/Contents/MacOS/cinnabar",
            "/tmp/zeno-profile",
        ),
        (
            Platform::Linux,
            "/work/cinnabar/target/debug/cinnabar",
            "/tmp/zeno-profile",
        ),
    ] {
        let mut env = environment(executable, "");
        env.home = None;
        env.user_root = Some(PathBuf::from(root));
        env.xdg_runtime_dir = Some(PathBuf::from("/run/user/1000"));
        let layout = InstallLayout::resolve(platform, &env).unwrap();
        let root = PathBuf::from(root);
        assert_eq!(layout.user_config_root, root.join("config"));
        assert_eq!(layout.user_data_root, root.join("data"));
        assert_eq!(layout.runtime_root, root.join("run"));
        assert!(!layout.resource_root.starts_with(&root));
        assert!(layout.auth_cache().starts_with(root.join("data")));
        assert!(
            layout
                .compiled_pack_cache_dir()
                .starts_with(root.join("data"))
        );
        for dir in [layout.shader_packs_dir(), layout.shader_pack_cache_dir()] {
            assert!(dir.starts_with(root.join("data")));
        }
        assert!(
            layout
                .skin_selection_file()
                .starts_with(root.join("config"))
        );
    }
}

#[test]
fn isolated_user_root_rejects_relative_and_empty_paths() {
    for platform in [Platform::Windows, Platform::Linux, Platform::MacOs] {
        for root in ["relative", ""] {
            let mut env = environment("/work/cinnabar/target/debug/cinnabar", "/home/dev");
            env.user_root = Some(PathBuf::from(root));
            assert!(matches!(
                InstallLayout::resolve(platform, &env),
                Err(LayoutError::InvalidUserRoot {
                    variable: "CINNABAR_USER_ROOT",
                    ..
                })
            ));
        }
    }
}

/// `make play` builds into `target/play`, which must still use the checkout's `.local`.
#[test]
fn play_profile_binary_uses_the_development_layout() {
    let layout = InstallLayout::resolve(
        Platform::Linux,
        &environment("/work/cinnabar/target/play/cinnabar", "/home/dev"),
    )
    .unwrap();
    assert_eq!(
        layout.runtime_root,
        PathBuf::from("/work/cinnabar/.local/run")
    );
}

#[test]
fn resolves_development_layout_from_target_ancestor() {
    let layout = InstallLayout::resolve(
        Platform::Linux,
        &environment("/work/cinnabar/target/debug/deps/client-test", "/home/dev"),
    )
    .unwrap();
    assert_eq!(
        layout.world_assets(),
        PathBuf::from("/work/cinnabar/.local/assets/compiled/vanilla-v2193.mcbea")
    );
    assert_eq!(
        layout.physics_registry,
        PathBuf::from("/work/cinnabar/.local/assets/compiled")
            .join(assets::carriers::physics_registry_basename())
    );
    assert_eq!(
        layout.runtime_root,
        PathBuf::from("/work/cinnabar/.local/run")
    );
    assert_eq!(
        layout.direct_socket_dir(41),
        PathBuf::from("/work/cinnabar/.local/cinnabar/direct-41")
    );
    assert_eq!(
        layout.connect_socket_dir(41, 3),
        PathBuf::from("/work/cinnabar/.local/cinnabar/connect-41-3")
    );
    assert_eq!(
        layout.catalog_file(41),
        PathBuf::from("/work/cinnabar/.local/cinnabar/catalog-41.json")
    );
    assert_eq!(
        layout.resource_pack_cache_dir(),
        PathBuf::from("/work/cinnabar/.local/resource-packs/v1/objects")
    );
}

#[test]
fn resolves_windows_app_layout_and_user_roots() {
    let mut env = environment("C:/Program Files/Cinnabar/cinnabar.exe", "C:/Users/dev");
    env.local_app_data = Some(PathBuf::from("C:/Users/dev/AppData/Local"));
    let layout = InstallLayout::resolve(Platform::Windows, &env).unwrap();
    assert_eq!(
        layout.compiled_assets,
        PathBuf::from("C:/Program Files/Cinnabar/resources/assets")
    );
    assert_eq!(
        layout.auth_cache(),
        PathBuf::from("C:/Users/dev/AppData/Local/Cinnabar/auth/microsoft-token.json")
    );
    // Downloaded art stays where earlier cores cached it, so upgrades keep their cache.
    assert_eq!(
        layout.launcher_artwork_dir(),
        PathBuf::from("C:/Users/dev/AppData/Local/Cinnabar/auth/catalog-cache/artwork")
    );
    assert_eq!(
        layout.resource_pack_cache_dir(),
        PathBuf::from("C:/Users/dev/AppData/Local/Cinnabar/resource-packs/v1/objects")
    );
}

#[test]
fn resolves_linux_xdg_layout() {
    let mut env = environment("/opt/cinnabar/bin/cinnabar", "/home/dev");
    env.xdg_config_home = Some(PathBuf::from("/cfg"));
    env.xdg_data_home = Some(PathBuf::from("/data"));
    env.xdg_runtime_dir = Some(PathBuf::from("/run/user/1000"));
    let layout = InstallLayout::resolve(Platform::Linux, &env).unwrap();
    assert_eq!(
        layout.compiled_assets,
        PathBuf::from("/opt/cinnabar/share/cinnabar/assets")
    );
    assert_eq!(
        layout.server_file(),
        PathBuf::from("/cfg/cinnabar/servers.json")
    );
    assert_eq!(
        layout.runtime_root,
        PathBuf::from("/run/user/1000/cinnabar")
    );
    assert_eq!(
        layout.resource_pack_cache_dir(),
        PathBuf::from("/data/cinnabar/resource-packs/v1/objects")
    );
}

#[test]
fn resolves_macos_app_bundle() {
    let layout = InstallLayout::resolve(
        Platform::MacOs,
        &environment(
            "/Applications/Cinnabar.app/Contents/MacOS/cinnabar",
            "/Users/dev",
        ),
    )
    .unwrap();
    assert_eq!(
        layout.compiled_assets,
        PathBuf::from("/Applications/Cinnabar.app/Contents/Resources/assets")
    );
    assert_eq!(
        layout.core_executable,
        PathBuf::from("/Applications/Cinnabar.app/Contents/MacOS/bedrock-core")
    );
    assert_eq!(
        layout.resource_pack_cache_dir(),
        PathBuf::from("/Users/dev/Library/Application Support/Cinnabar/resource-packs/v1/objects")
    );
}

#[test]
fn installed_layouts_fail_without_required_identity_or_user_roots() {
    let mut linux = environment("/opt/cinnabar/bin/cinnabar", "/home/dev");
    linux.home = None;
    assert_eq!(
        InstallLayout::resolve(Platform::Linux, &linux),
        Err(LayoutError::MissingUserHome { platform: "Linux" })
    );

    let windows = environment("C:/Cinnabar/cinnabar.exe", "C:/Users/dev");
    assert_eq!(
        InstallLayout::resolve(Platform::Windows, &windows),
        Err(LayoutError::MissingLocalAppData)
    );

    let mut missing_executable = linux;
    missing_executable.executable = PathBuf::new();
    assert_eq!(
        InstallLayout::resolve(Platform::Linux, &missing_executable),
        Err(LayoutError::MissingExecutable)
    );
}

#[test]
fn installed_layouts_never_accept_relative_user_roots() {
    let mut windows = environment("C:/Cinnabar/cinnabar.exe", "C:/Users/dev");
    windows.local_app_data = Some(PathBuf::from("relative/local"));
    assert!(matches!(
        InstallLayout::resolve(Platform::Windows, &windows),
        Err(LayoutError::InvalidUserRoot {
            variable: "LOCALAPPDATA",
            ..
        })
    ));

    let mac = environment(
        "/Applications/Cinnabar.app/Contents/MacOS/cinnabar",
        "relative/home",
    );
    assert!(matches!(
        InstallLayout::resolve(Platform::MacOs, &mac),
        Err(LayoutError::InvalidUserRoot {
            variable: "HOME",
            ..
        })
    ));

    let mut linux = environment("/opt/cinnabar/bin/cinnabar", "/home/dev");
    linux.xdg_config_home = Some(PathBuf::from("relative/config"));
    linux.xdg_data_home = Some(PathBuf::new());
    linux.xdg_runtime_dir = Some(PathBuf::from("relative/run"));
    let layout = InstallLayout::resolve(Platform::Linux, &linux).unwrap();
    assert_eq!(
        layout.user_config_root,
        PathBuf::from("/home/dev/.config/cinnabar")
    );
    assert_eq!(
        layout.user_data_root,
        PathBuf::from("/home/dev/.local/share/cinnabar")
    );
    assert_eq!(
        layout.runtime_root,
        PathBuf::from("/home/dev/.local/share/cinnabar/run")
    );
}

#[test]
fn absolute_linux_xdg_roots_do_not_require_home() {
    let mut linux = environment("/opt/cinnabar/bin/cinnabar", "/unused");
    linux.home = None;
    linux.xdg_config_home = Some(PathBuf::from("/cfg"));
    linux.xdg_data_home = Some(PathBuf::from("/data"));
    linux.xdg_runtime_dir = Some(PathBuf::from("/run/user/1000"));
    let layout = InstallLayout::resolve(Platform::Linux, &linux).unwrap();
    assert_eq!(layout.user_config_root, PathBuf::from("/cfg/cinnabar"));
    assert_eq!(layout.user_data_root, PathBuf::from("/data/cinnabar"));
    assert_eq!(
        layout.runtime_root,
        PathBuf::from("/run/user/1000/cinnabar")
    );
}

#[test]
fn malformed_macos_path_never_guesses_an_app_bundle() {
    let mac_environment = environment("/opt/cinnabar/bin/cinnabar", "/Users/dev");
    assert!(matches!(
        InstallLayout::resolve(Platform::MacOs, &mac_environment),
        Err(LayoutError::InvalidMacOsBundle(_))
    ));
    let linux_environment = environment("/opt/cinnabar/cinnabar", "/home/dev");
    assert!(matches!(
        InstallLayout::resolve(Platform::Linux, &linux_environment),
        Err(LayoutError::InvalidLinuxLayout(_))
    ));
}

#[test]
fn installed_layout_without_bundled_carriers_uses_prepared_assets() {
    let mut env = environment("/nonexistent/opt/cinnabar/bin/cinnabar", "/home/dev");
    env.xdg_data_home = Some(PathBuf::from("/data"));
    env.xdg_config_home = Some(PathBuf::from("/cfg"));
    let layout = InstallLayout::resolve(Platform::Linux, &env)
        .unwrap()
        .with_prepared_assets();
    assert!(layout.is_installed());
    assert_eq!(
        layout.vanilla_pack_dir(),
        layout.user_data_root.join(vanilla_pack_relative())
    );
    assert_eq!(
        layout.compiled_assets,
        PathBuf::from("/data/cinnabar/assets/compiled")
    );
    assert_eq!(
        layout.prep_kit(),
        PathBuf::from("/nonexistent/opt/cinnabar/share/cinnabar/prep-kit")
    );
}

#[test]
fn development_layout_is_not_installed() {
    let layout = InstallLayout::resolve(
        Platform::Linux,
        &environment("/work/cinnabar/target/release/cinnabar", "/home/dev"),
    )
    .unwrap()
    .with_prepared_assets();
    assert!(!layout.is_installed());
    assert_eq!(
        layout.vanilla_pack_dir(),
        layout.resource_root.join(vanilla_pack_relative())
    );
    assert_eq!(
        layout.compiled_assets,
        PathBuf::from("/work/cinnabar/.local/assets/compiled")
    );
}
