//! A Mods tab library with and without self-declared component metadata.

use super::{Arc, Fixture, settings};
use launcher::mods::{ActiveMod, InstalledMod, Library, Permission, Snapshot, Status};
use ui::mod_metadata::ModMetadata;

/// Three components; `metadata` decides whether they describe themselves.
fn view(metadata: bool) -> Fixture {
    let card = |id, file: &str, declared: ModMetadata| InstalledMod {
        id,
        file: file.into(),
        bytes: 182_272,
        permissions: vec![Permission {
            name: "time override".into(),
            description: "Lets the mod fix the local time of day.".into(),
            enabled: true,
            requested: Some(true),
            requested_allowed: true,
            sensitive: false,
        }],
        metadata: if metadata {
            declared
        } else {
            ModMetadata::default()
        },
    };
    let mut view = settings(launcher::mods::SECTION);
    view.mods = Arc::new(Snapshot {
        library: Library {
            installed: vec![
                card(
                    1,
                    "zoom_plus-1.2.0.wasm",
                    ModMetadata {
                        name: Some("Zoom Plus".into()),
                        description: Some(
                            "Smooth, scroll-adjustable zoom on a key of your choice.".into(),
                        ),
                        version: Some("1.2.0".into()),
                        authors: Some("Example Author".into()),
                        licenses: Some("MIT OR Apache-2.0".into()),
                        icon: Some(icon()),
                        update_repository: None,
                        update_url: None,
                    },
                ),
                card(
                    2,
                    "minimap.wasm",
                    ModMetadata {
                        name: Some("Minimap".into()),
                        version: Some("0.4.1".into()),
                        ..ModMetadata::default()
                    },
                ),
                card(3, "fps-overlay.wasm", ModMetadata::default()),
            ],
            active: vec![ActiveMod {
                id: 1,
                status: Status::Running,
            }],
            ..Library::default()
        },
        details_expanded: Some(1),
        ..Snapshot::default()
    });
    Fixture::Menu(Box::new(view))
}

/// Cards named by their declared metadata, with one embedded icon.
pub fn declared() -> Fixture {
    view(true)
}

/// The same files without metadata: file names and placeholder icons.
pub fn undeclared() -> Fixture {
    view(false)
}

/// Original procedural art: a lens ring on a diagonal gradient.
fn icon() -> Arc<[u8]> {
    const SIDE: u32 = 64;
    let image = image::RgbaImage::from_fn(SIDE, SIDE, |x, y| {
        let (dx, dy) = (x as f32 - 27.0, y as f32 - 27.0);
        let distance = (dx * dx + dy * dy).sqrt();
        let handle = x > 38 && y > 38 && x.abs_diff(y) < 5 && x < 58;
        if (14.0..19.0).contains(&distance) || handle {
            image::Rgba([245, 245, 240, 255])
        } else {
            let t = (x + y) as f32 / (2 * SIDE) as f32;
            image::Rgba([
                (200.0 - 120.0 * t) as u8,
                (60.0 + 40.0 * t) as u8,
                (40.0 + 150.0 * t) as u8,
                255,
            ])
        }
    });
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .expect("encode fixture icon");
    png.into()
}

/// A sample library for permission summaries, consent and both customization modes.
fn permissions(customizing: bool, show_all: bool, consent: bool, declared: bool) -> Fixture {
    let Fixture::Menu(mut view) = view(true) else {
        unreachable!()
    };
    let snapshot = Arc::make_mut(&mut view.mods);
    snapshot.library.installed.truncate(1);
    snapshot.details_expanded = None;
    snapshot.customizing = customizing.then_some(1);
    snapshot.show_all_permissions = show_all;
    snapshot.consent = consent.then_some(1);
    snapshot.library.installed[0].permissions = [
        (
            "HUD",
            "Allows bounded HUD cards and a custom crosshair.",
            false,
            false,
        ),
        (
            "Camera rotation",
            "Allows bounded camera rotation and camera rigs.",
            true,
            true,
        ),
        (
            "Controls",
            "Allows key bindings and the mod settings panel.",
            true,
            true,
        ),
        (
            "Settings file",
            "Allows this mod's companion settings file.",
            true,
            false,
        ),
        (
            "Item use",
            "Allows air use after slot changes and while attacking.",
            false,
            true,
        ),
        (
            "Attack reach",
            "Allows bounded attack range and held attack requests.",
            false,
            true,
        ),
        (
            "Packet delay",
            "Allows bounded post-login packet delay.",
            false,
            true,
        ),
        (
            "Movement",
            "Allows physics snapshots and one-shot jumps.",
            false,
            true,
        ),
    ]
    .into_iter()
    .map(|(name, description, requested, sensitive)| Permission {
        name: name.into(),
        description: description.into(),
        enabled: requested && !consent,
        requested: declared.then_some(requested),
        requested_allowed: requested && !consent,
        sensitive,
    })
    .collect();
    if customizing {
        view.focused_action = Some(launcher::menu::MenuAction::Mods(
            launcher::mods::Action::Permission(1, 1, false),
        ));
        view.navigation_focus_visible = true;
    }
    Fixture::Menu(view)
}

/// Collapsed actions, requested-grant count and allowed gameplay warnings.
pub fn collapsed() -> Fixture {
    permissions(false, false, false, true)
}

/// A first-enable card waiting for explicit requested-grant consent.
pub fn consent() -> Fixture {
    permissions(false, false, true, true)
}

/// A compact grid limited to this sample mod's three requests.
pub fn requested_only() -> Fixture {
    permissions(true, false, false, true)
}

/// Requested permissions precede the optional permissions in the expanded grid.
pub fn show_all() -> Fixture {
    permissions(true, true, false, true)
}

/// A component without a declaration can only customize its grants.
pub fn undeclared_permissions() -> Fixture {
    permissions(false, false, true, false)
}
