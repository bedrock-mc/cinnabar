//! The pre-game window stack shared by the first-run setup window and the crash window: a winit
//! window over Cinnabar's built-in panorama with an OreUI overlay drawn on the CPU in the open UI
//! font. It needs no game assets, so it works before setup prepares any and after the game dies.
//! Each window runs in its own process: winit allows one event loop per process, and the game
//! opens its own.

pub mod canvas;
pub mod gpu;
pub mod input;
pub mod startup;
pub mod view;
pub mod window;

use std::{fs, path::PathBuf, time::Duration};

use anyhow::{Context, Result};
use assets::carriers::{FONT_MANIFEST, Sources};
use launcher::{install_layout::InstallLayout, menu::settings_options::SettingsOptions};
use serde::Deserialize;
use ui::oreui_theme::Appearance;
use winit::dpi::PhysicalSize;

use canvas::Image;

/// How much the panorama is dimmed behind the overlay.
pub const PANORAMA_TINT: [f32; 4] = [0.0, 0.0, 0.0, 0.3];
/// The overlay repaints at most this often; the panorama animates every frame.
pub const OVERLAY_INTERVAL: Duration = Duration::from_millis(66);
/// Where packaging installs the pinned UI font, under the resource root.
const INSTALLED_FONT_DIR: &str = "fonts";
/// Where a checkout keeps the pinned UI font, relative to its root.
const CHECKOUT_FONT_DIR: &str = "assets/fonts";
/// Cinnabar's own panorama behind the bootstrap windows, in pack face order (-Z, +X, +Z, -X, up,
/// down). Menus use the pack's faces once it is prepared.
const BUILT_IN_FACES: [&[u8]; 6] = [
    include_bytes!("../../../assets/panorama/panorama_0.jpg"),
    include_bytes!("../../../assets/panorama/panorama_1.jpg"),
    include_bytes!("../../../assets/panorama/panorama_2.jpg"),
    include_bytes!("../../../assets/panorama/panorama_3.jpg"),
    include_bytes!("../../../assets/panorama/panorama_4.jpg"),
    include_bytes!("../../../assets/panorama/panorama_5.jpg"),
];

/// The UI font (Cinnangles Sans) for `layout`, parsed for drawing; errors name the font file.
pub fn text(layout: &InstallLayout) -> Result<canvas::Text> {
    let (path, bytes) = font(layout)?;
    canvas::Text::from_bytes(&bytes)
        .with_context(|| format!("the UI font {} does not parse", path.display()))
}

/// The UI font's path and bytes for `layout`, as the pinned font manifest names it.
///
/// An install reads the prep kit's copy of the manifest and the font packaging put under its
/// resources. A development layout keeps its resources in the checkout's `.local`, so it reads the
/// checkout's own manifest and font.
fn font(layout: &InstallLayout) -> Result<(PathBuf, Vec<u8>)> {
    let path = if layout.is_installed() {
        let file = ui_font_file(&Sources::Kit(layout.prep_kit()))?;
        layout.resource_root.join(INSTALLED_FONT_DIR).join(file)
    } else {
        let checkout = layout
            .resource_root
            .parent()
            .unwrap_or(&layout.resource_root);
        let sources = Sources::Checkout(checkout.to_path_buf());
        let file = ui_font_file(&sources)?;
        sources.resolve(&format!("{CHECKOUT_FONT_DIR}/{file}"))
    };
    let bytes = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
    Ok((path, bytes))
}

/// The UI font's file name, from the font manifest in `sources`.
fn ui_font_file(sources: &Sources) -> Result<String> {
    #[derive(Deserialize)]
    struct FontSource {
        font_file: String,
    }
    let path = sources.resolve(FONT_MANIFEST);
    let bytes = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
    let font: FontSource =
        serde_json::from_slice(&bytes).with_context(|| format!("parse {}", path.display()))?;
    Ok(font.font_file)
}

/// Decodes the built-in panorama faces; `None` only if the embedded art is corrupt.
pub fn built_in_faces() -> Option<render_model::PanoramaFaces> {
    render_model::panorama::decode_faces(|face| Some(BUILT_IN_FACES[face]))
}

/// Decodes the product's title logo; `None` only if the embedded art is corrupt.
pub fn decode_logo() -> Option<Image> {
    let image = image::load_from_memory(launcher::branding::TITLE).ok()?;
    let rgba = image.into_rgba8();
    Some(Image {
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw(),
    })
}

/// Opens connected controllers, or reports why controller input is unavailable.
pub fn controllers() -> Option<gilrs::Gilrs> {
    gilrs::Gilrs::new()
        .map_err(|error| tracing::warn!("bootstrap controller input unavailable: {error}"))
        .ok()
}

/// The saved menu choices the bootstrap windows honour without loading a resource pack. The
/// default is the menus' own default.
#[derive(Default)]
pub struct Preferences {
    /// Controller confirm/back swapping.
    pub settings: SettingsOptions,
    /// Light or dark OreUI.
    pub appearance: Appearance,
    /// The menus' GUI-scale offset.
    pub gui_scale_offset: i8,
}

impl Preferences {
    /// Reads the menu settings and video settings `layout` keeps; unreadable files give defaults.
    pub fn load(layout: &InstallLayout) -> Self {
        let settings = SettingsOptions::load(
            &layout
                .server_file()
                .with_file_name(launcher::menu::settings_options::SETTINGS_FILE),
        );
        let appearance = Appearance::from_dark(settings.oreui_dark_mode());
        let gui_scale_offset = launcher_host::video_settings::load(&layout.user_config_root)
            .unwrap_or_default()
            .gui_scale_offset;
        Self {
            settings,
            appearance,
            gui_scale_offset,
        }
    }

    /// Physical pixels per rem for a window of `size`, from the same GUI-scale authority as the
    /// menus.
    pub fn rem(&self, size: PhysicalSize<u32>) -> f32 {
        f32::from(
            ui::DesktopGuiScale::for_window([size.width, size.height])
                .scale_for_offset(self.gui_scale_offset),
        ) * ui::oreui_theme::GUI_PIXELS_PER_REM
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use launcher::install_layout::{InstallEnvironment, Platform};

    use super::*;

    #[test]
    fn built_in_art_decodes() {
        assert!(decode_logo().is_some());
        assert!(built_in_faces().is_some());
    }

    /// The host's layout for `executable` under `root`, with the user roots isolated there too.
    fn layout(root: &Path, executable: &str) -> InstallLayout {
        InstallLayout::resolve(
            if cfg!(windows) {
                Platform::Windows
            } else {
                Platform::Linux
            },
            &InstallEnvironment {
                executable: root.join(executable),
                user_root: Some(root.join("user")),
                home: None,
                local_app_data: None,
                xdg_config_home: None,
                xdg_data_home: None,
                xdg_runtime_dir: None,
            },
        )
        .unwrap()
    }

    fn write(path: &Path, bytes: &[u8]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    #[test]
    fn installs_and_checkouts_read_the_font_their_manifest_names() {
        let dir = tempfile::tempdir().unwrap();
        let installed = layout(dir.path(), "opt/bin/cinnabar");
        let development = layout(dir.path(), "checkout/target/debug/cinnabar");
        assert!(installed.is_installed() && !development.is_installed());
        assert!(font(&installed).is_err() && font(&development).is_err());

        let manifest = br#"{"family":"Cinnangles Sans","font_file":"Pinned.ttf"}"#;
        let kit = Sources::Kit(installed.prep_kit());
        write(&kit.resolve(FONT_MANIFEST), manifest);
        let installed_font: PathBuf = installed
            .resource_root
            .join(INSTALLED_FONT_DIR)
            .join("Pinned.ttf");
        write(&installed_font, b"installed");
        let checkout = Sources::Checkout(dir.path().join("checkout"));
        write(&checkout.resolve(FONT_MANIFEST), manifest);
        write(
            &checkout.resolve(&format!("{CHECKOUT_FONT_DIR}/Pinned.ttf")),
            b"checkout",
        );

        assert_eq!(font(&installed).unwrap().1, b"installed");
        assert_eq!(font(&development).unwrap().1, b"checkout");
        let unparsable = text(&installed).err().unwrap().to_string();
        assert!(unparsable.contains("Pinned.ttf"), "{unparsable}");
    }
}
