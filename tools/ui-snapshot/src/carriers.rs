//! Reads only compiled artwork and font fixtures, never account or server data.

use std::{fs::File, io::Read, path::Path, sync::Arc};

use assets::{RuntimeFontCatalog, RuntimeLangCatalog, RuntimeUiAssets, carriers};
use jsonui_editor::raster::Page;

/// Missing local fixtures skip explicitly; corrupt or unreadable fixtures fail.
#[derive(Debug)]
pub enum Error {
    Missing(String),
    Invalid(String),
}

impl From<String> for Error {
    fn from(value: String) -> Self {
        Self::Invalid(value)
    }
}

/// The validated fonts, carrier and pages for one backend.
pub struct Carriers {
    pub font: Arc<RuntimeFontCatalog>,
    pub ui: Arc<RuntimeUiAssets>,
    pub lang: RuntimeLangCatalog,
    pub pages: Vec<Page>,
    pub solid_page: u16,
    pub ui_page: u16,
}

/// Reads bounded carrier bytes, distinguishing absent fixtures from broken ones.
pub fn read(path: &Path, limit: usize) -> Result<Vec<u8>, Error> {
    let file = File::open(path).map_err(|error| {
        let message = format!("{}: {error}; make assets", path.display());
        if error.kind() == std::io::ErrorKind::NotFound {
            Error::Missing(message)
        } else {
            Error::Invalid(message)
        }
    })?;
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| Error::Invalid(format!("{}: {error}", path.display())))?;
    if bytes.len() > limit {
        return Err(Error::Invalid(format!(
            "{} exceeds its carrier bound",
            path.display()
        )));
    }
    Ok(bytes)
}

/// Rejects missing, damaged or unpinned artwork before the optional runtime loader runs.
pub fn validate_oreui(dir: &Path) -> Result<(), Error> {
    use assets::oreui_icons::{
        MAX_OREUI_ICONS_BYTES, OREUI_ICONS_SOURCE_MANIFEST, decode_oreui_icons,
    };

    let path = dir.join(carriers::OREUI_ICONS.output);
    let bytes = read(&path, MAX_OREUI_ICONS_BYTES)?;
    let manifest = assets::canonical_source_manifest_sha256(OREUI_ICONS_SOURCE_MANIFEST);
    decode_oreui_icons(&bytes, manifest)
        .map_err(|error| Error::Invalid(format!("{}: {error}; make assets", path.display())))?;
    Ok(())
}

/// Validates the same pinned manifests and installs OreUI's shipped font faces.
pub fn load(dir: &Path, menu: bool) -> Result<Carriers, Error> {
    let profile = carriers::FONT.font_face.unwrap();
    let manifest = assets::canonical_source_manifest_sha256(profile.manifest);
    let font = RuntimeFontCatalog::decode(
        &read(
            &dir.join(carriers::FONT.output),
            assets::MAX_FONT_CARRIER_BYTES,
        )?,
        manifest,
    )
    .map_err(|error| Error::Invalid(error.to_string()))?;
    let font = if menu {
        for face in ui::oreui_theme::OreUiFont::ALL {
            let bytes = read(
                &dir.join(face.carrier().output),
                assets::MAX_FONT_CARRIER_BYTES,
            )?;
            let manifest = assets::canonical_source_manifest_sha256(
                face.carrier().font_face.unwrap().manifest,
            );
            assets::decode_font_face_bundle(&bytes, manifest).map_err(|error| {
                Error::Invalid(format!(
                    "{}: {error}",
                    dir.join(face.carrier().output).display()
                ))
            })?;
        }
        oreui::fonts::install(Arc::new(font), dir)
    } else {
        Arc::new(font)
    };
    let font = mod_ui::fonts::install(font)?;
    let ui = RuntimeUiAssets::decode(&read(
        &dir.join(carriers::UI.output),
        assets::MAX_UI_CARRIER_BYTES,
    )?)
    .map_err(|error| Error::Invalid(error.to_string()))?;
    let expected =
        assets::canonical_source_manifest_sha256(assets::VANILLA_SOURCE_MANIFEST.as_bytes());
    if ui.source_manifest_sha256() != expected {
        return Err(Error::Invalid(
            "UI carrier has a different pinned source; make assets".into(),
        ));
    }
    let lang = RuntimeLangCatalog::decode(&read(
        &dir.join(carriers::LANG.output),
        assets::MAX_LANG_CARRIER_BYTES,
    )?)
    .map_err(|error| error.to_string())?;
    if lang.source_manifest_sha256() != expected
        || lang.lang_source_sha256() != assets::VANILLA_EN_US_LANG_SHA256
    {
        return Err(Error::Invalid(
            "language carrier has a different pinned source; make assets".into(),
        ));
    }
    let mut pages: Vec<Page> = font
        .pages()
        .iter()
        .map(|page| Page {
            width: page.width,
            height: page.height,
            rgba: (0..page.width as usize * page.height as usize)
                .flat_map(|index| page.pixels.texel(index).unwrap())
                .collect(),
        })
        .collect();
    let solid_page = pages.len() as u16;
    pages.push(Page::white());
    let ui_page = pages.len() as u16;
    pages.extend(ui.atlas_pages().iter().map(|page| Page {
        width: page.width,
        height: page.height,
        rgba: Arc::clone(&page.rgba8),
    }));
    Ok(Carriers {
        font,
        ui: Arc::new(ui),
        lang,
        pages,
        solid_page,
        ui_page,
    })
}

/// Installs the same runtime Steve fallback used by the client, without embedding game art.
pub fn install_default_skin(dir: &Path) -> Result<(), Error> {
    let bytes = read(
        &dir.join(carriers::STARTER_SKINS.output),
        assets::starter_skins::MAX_STARTER_SKINS_BYTES,
    )?;
    let skins = assets::starter_skins::decode_starter_skins(&bytes).map_err(|e| e.to_string())?;
    let default = skins
        .skins
        .first()
        .ok_or_else(|| Error::Invalid("starter skin carrier has no default".into()))?;
    render_model::install_default_player_skin(default.rgba8.to_vec().into());
    Ok(())
}
