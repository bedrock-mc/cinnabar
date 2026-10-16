//! Renders the window icons from the shipped logo and embeds the built web UI, or a placeholder
//! page when only Rust is being checked.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

const REQUIRE_UI: &str = "CINNABAR_REQUIRE_LAUNCHER_UI";
const LOGO: &str = "../../packaging/icons/cinnabar.svg";

const CHANGELOG: &str = "CINNABAR_CHANGELOG";

fn main() {
    println!("cargo::rustc-check-cfg=cfg(webview)");
    embed_changelog();
    let linux = env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "linux");
    if linux && env::var_os("CARGO_FEATURE_LINUX_WEBVIEW").is_none() {
        return;
    }
    println!("cargo:rustc-cfg=webview");
    let manifest =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("Cargo sets the manifest dir"));
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR"));
    println!("cargo:rerun-if-changed=ui/dist");
    println!("cargo:rerun-if-changed={LOGO}");
    println!("cargo:rerun-if-env-changed={REQUIRE_UI}");
    let (png, ico) = render_icons(&manifest.join(LOGO), &out);
    let mut config = serde_json::json!({ "bundle": { "icon": [png, ico] } });
    if !manifest.join("ui/dist/index.html").is_file() {
        assert!(
            env::var_os(REQUIRE_UI).is_none(),
            "the launcher UI is not built; run `make launcher-ui`"
        );
        let placeholder = out.join("placeholder-ui");
        fs::create_dir_all(&placeholder).expect("create the placeholder UI directory");
        fs::write(
            placeholder.join("index.html"),
            "<!doctype html><meta charset=utf-8><title>Cinnabar</title><p>The launcher UI was not built.</p>",
        )
        .expect("write the placeholder UI");
        config["build"] = serde_json::json!({ "frontendDist": placeholder });
    }
    let config = config.to_string();
    // SAFETY: build scripts are single-threaded before tauri-build reads the variable.
    unsafe { env::set_var("TAURI_CONFIG", &config) };
    println!("cargo:rustc-env=TAURI_CONFIG={config}");
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .windows_attributes(tauri_build::WindowsAttributes::new().window_icon_path(&ico)),
    )
    .expect("tauri-build");
}

/// Rasterizes the logo into a PNG and a multi-size ICO under `out`.
fn render_icons(logo: &Path, out: &Path) -> (PathBuf, PathBuf) {
    use resvg::{tiny_skia, usvg};
    let svg = fs::read(logo).expect("read the logo");
    let tree = usvg::Tree::from_data(&svg, &usvg::Options::default()).expect("parse the logo");
    let rgba = |size: u32| {
        let mut pixmap = tiny_skia::Pixmap::new(size, size).expect("icon size");
        let scale = size as f32 / tree.size().width().max(tree.size().height());
        resvg::render(
            &tree,
            tiny_skia::Transform::from_scale(scale, scale),
            &mut pixmap.as_mut(),
        );
        pixmap
            .pixels()
            .iter()
            .flat_map(|pixel| {
                let color = pixel.demultiply();
                [color.red(), color.green(), color.blue(), color.alpha()]
            })
            .collect::<Vec<u8>>()
    };
    let mut icons = ico::IconDir::new(ico::ResourceType::Icon);
    for size in [16, 32, 48, 64, 128, 256] {
        let image = ico::IconImage::from_rgba_data(size, size, rgba(size));
        icons.add_entry(ico::IconDirEntry::encode(&image).expect("encode an icon size"));
    }
    let ico_path = out.join("icon.ico");
    icons
        .write(fs::File::create(&ico_path).expect("create icon.ico"))
        .expect("write icon.ico");
    let png_path = out.join("icon.png");
    let image = ico::IconImage::from_rgba_data(512, 512, rgba(512));
    image
        .write_png(fs::File::create(&png_path).expect("create icon.png"))
        .expect("write icon.png");
    (png_path, ico_path)
}

/// Embeds the release highlights packaging generated for this build, or `null` without them.
fn embed_changelog() {
    println!("cargo:rerun-if-env-changed={CHANGELOG}");
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR"));
    let changelog = match env::var_os(CHANGELOG) {
        Some(path) => {
            println!("cargo:rerun-if-changed={}", Path::new(&path).display());
            fs::read_to_string(&path).expect("read the generated changelog")
        }
        None => "null".to_owned(),
    };
    fs::write(out.join("changelog.json"), changelog).expect("embed the changelog");
}
