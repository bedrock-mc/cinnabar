//! Session-only catalog changes; browser imports never claim to be saved to disk.

use super::*;
use launcher::skin_import::{self, picked::PickedFile};

/// Creates a catalog from an already loaded appearance without touching a filesystem.
pub(crate) fn load(fallback: &LocalPlayerSkin) -> DressingRoomView {
    DressingRoomView {
        skins: vec![DressingRoomSkin {
            id: format!("{SESSION_SKIN_ID_PREFIX}{}", fallback.skin_id),
            name: "Current skin".into(),
            // Retain the current login identity until an actual selection changes it.
            path: String::new(),
            imported: false,
            model: fallback.model(),
            engine_version: fallback.engine_version.clone(),
            skin: fallback.standard_skin(),
        }]
        .into(),
        selected: Some(0),
        capes: fallback
            .cape
            .iter()
            .map(|cape| launcher::dressing_room::DressingRoomCape {
                id: "current".into(),
                name: "Current cape".into(),
                path: String::new(),
                cape: cape.clone(),
                imported: false,
            })
            .collect::<Vec<_>>()
            .into(),
        selected_cape: fallback.cape.as_ref().map(|_| 0),
        selected_cosmetics: fallback
            .cosmetics
            .iter()
            .map(|item| (item.descriptor().slot.clone(), Some(item.id().to_owned())))
            .collect(),
        ..Default::default()
    }
}

/// Uses the retained session skin when optional starter skins are not loaded.
pub(crate) fn deletion_fallback(skins: &[DressingRoomSkin]) -> Result<usize, String> {
    super::deletion_fallback(skins).or_else(|error| {
        skins
            .iter()
            .position(|entry| !entry.imported && entry.id.starts_with(SESSION_SKIN_ID_PREFIX))
            .ok_or(error)
    })
}

/// Keeps a partially decoded archive private until every entry has passed validation.
pub(crate) struct Import {
    next: DressingRoomView,
    entries: Inputs,
    input: Option<skin_import::SkinPackEntry>,
    prepared: Option<custom::PreparedSkin>,
}

enum Inputs {
    Pack(skin_import::SkinPackReader<Vec<u8>>),
    Loose(Option<skin_import::SkinPackEntry>),
}

impl Import {
    /// Validates names and input sizes again at the Rust catalog boundary.
    pub(crate) fn begin(
        view: &DressingRoomView,
        mut files: Vec<PickedFile>,
    ) -> Result<Self, String> {
        skin_import::picked::admit(
            &files
                .iter()
                .map(|file| (file.name.as_str(), file.bytes.len() as u64))
                .collect::<Vec<_>>(),
            false,
        )
        .map_err(|error| error.to_string())?;
        let entries = if files.len() == 1 && files[0].name.to_ascii_lowercase().ends_with(".mcpack")
        {
            Inputs::Pack(
                skin_import::SkinPackReader::new(files.remove(0).bytes)
                    .map_err(|error| error.to_string())?,
            )
        } else {
            Inputs::Loose(Some(
                skin_import::picked::loose(files).map_err(|error| error.to_string())?,
            ))
        };
        Ok(Self {
            next: view.clone(),
            entries,
            input: None,
            prepared: None,
        })
    }

    /// Separates archive parsing, image decode and model validation into independent frame slices.
    pub(crate) fn step(&mut self) -> Result<bool, String> {
        let prepared = if let Some(prepared) = self.prepared.take() {
            prepared.validate_model()?;
            prepared
        } else if let Some(input) = self.input.take() {
            self.prepared = Some(custom::decode_entry(input, |model| {
                geometry(&self.next, model)
            })?);
            return Ok(false);
        } else {
            self.input = match &mut self.entries {
                Inputs::Pack(pack) => pack.next().transpose().map_err(|error| error.to_string())?,
                Inputs::Loose(entry) => entry.take(),
            };
            return Ok(self.input.is_none());
        };
        if let Some(index) = self
            .next
            .skins
            .iter()
            .position(|entry| entry.id == prepared.entry.id)
        {
            self.next.selected = Some(index);
            return Ok(false);
        }
        if self
            .next
            .skins
            .iter()
            .filter(|entry| entry.imported)
            .count()
            >= MAX_IMPORTED_ITEMS
        {
            return Err(format!(
                "The skin library already contains {MAX_IMPORTED_ITEMS} imported skins."
            ));
        }
        let mut skins = self.next.skins.to_vec();
        skins.push(prepared.entry);
        self.next.selected = Some(skins.len() - 1);
        self.next.skins = skins.into();
        Ok(false)
    }

    /// Returns the immutable replacement after the caller has exhausted all input steps.
    pub(crate) fn finish(self) -> DressingRoomView {
        self.next
    }
}

/// Finds installed model inputs; optional starter assets are never fabricated.
pub(crate) fn geometry(
    view: &DressingRoomView,
    model: SkinModel,
) -> Result<Option<Arc<render_api::SkinGeometrySource>>, String> {
    match view
        .skins
        .iter()
        .find(|entry| entry.model == model)
        .and_then(|entry| entry.skin.geometry.clone())
    {
        Some(source) => Ok(Some(source)),
        None if model == SkinModel::Classic => Ok(None),
        None => Err("The selected model is unavailable until its skin assets are loaded.".into()),
    }
}

/// Imports a cape using the native decoder and content identity without retaining encoded bytes.
pub(crate) fn import_cape(
    view: &mut DressingRoomView,
    mut files: Vec<PickedFile>,
) -> Result<(), String> {
    skin_import::picked::admit(
        &files
            .iter()
            .map(|file| (file.name.as_str(), file.bytes.len() as u64))
            .collect::<Vec<_>>(),
        true,
    )
    .map_err(|error| error.to_string())?;
    let file = files.remove(0);
    let cape = capes::decode(&file.bytes)?;
    let digest = Sha256::digest(&file.bytes);
    let id = format!("cape:{digest:x}");
    if let Some(index) = view.capes.iter().position(|entry| entry.id == id) {
        view.selected_cape = Some(index);
        return Ok(());
    }
    if view.capes.iter().filter(|entry| entry.imported).count() >= MAX_IMPORTED_ITEMS {
        return Err(format!(
            "The cape library already contains {MAX_IMPORTED_ITEMS} imported capes."
        ));
    }
    let mut capes = view.capes.to_vec();
    capes.push(launcher::dressing_room::DressingRoomCape {
        id,
        name: file.name[..file.name.len() - ".png".len()].to_owned(),
        path: format!("{digest:x}.png"),
        cape,
        imported: true,
    });
    view.selected_cape = Some(capes.len() - 1);
    view.capes = capes.into();
    Ok(())
}

/// Validates a new library label with the shared native text contract.
pub(crate) fn valid_name(name: &str) -> Result<&str, String> {
    edits::valid_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Encodes an original fixture image accepted by the shared PNG decoder.
    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(width, height, image::Rgba([11, 170, 220, 255]))
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    }

    #[test]
    fn memory_import_is_incremental_atomic_and_matches_native_identity() {
        let fallback = LocalPlayerSkin::generated_default("memory fixture");
        let initial = load(&fallback);
        assert_eq!(
            crate::player_skin::selected_skin_id(initial.selected_skin().unwrap()),
            fallback.skin_id
        );
        let bytes = png(
            render_api::CLASSIC_SKIN_SIDE as u32,
            render_api::CLASSIC_SKIN_SIDE as u32,
        );
        let mut import = Import::begin(
            &initial,
            vec![PickedFile {
                name: "Skin.PNG".into(),
                bytes: bytes.clone(),
            }],
        )
        .unwrap();
        assert!(!import.step().unwrap());
        assert_eq!(initial.skins.len(), 1, "no partial import is published");
        while !import.step().unwrap() {}
        let memory = import.finish();
        let directory = tempfile::tempdir().unwrap();
        let layout = super::super::tests::layout();
        let source = directory.path().join("Skin.PNG");
        fs::write(&source, bytes).unwrap();
        let mut native = initial.clone();
        custom::import(&layout, &mut native, &source).unwrap();
        let native_skin = native.selected_skin().unwrap();
        let memory_skin = memory.selected_skin().unwrap();
        assert_eq!(native_skin.skin, memory_skin.skin);
        assert_eq!(native_skin.engine_version, memory_skin.engine_version);
        assert_eq!(
            crate::player_skin::selected_skin_id(native_skin),
            crate::player_skin::selected_skin_id(memory_skin)
        );
    }

    #[test]
    fn a_pack_failure_after_a_valid_entry_never_publishes_partial_imports() {
        use std::io::Write;
        let fallback = LocalPlayerSkin::generated_default("pack fixture");
        let view = load(&fallback);
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let metadata = serde_json::json!({"skins": [
            {"localization_name":"First", "texture":"first.png", "geometry":SkinModel::Classic.geometry(), "type":"free"},
            {"localization_name":"Broken", "texture":"broken.png", "geometry":SkinModel::Classic.geometry(), "type":"free"}
        ]}).to_string();
        for (name, bytes) in [
            (
                "manifest.json",
                br#"{"modules":[{"type":"skin_pack"}]}"#.to_vec(),
            ),
            ("skins.json", metadata.into_bytes()),
            (
                "first.png",
                png(
                    render_api::CLASSIC_SKIN_SIDE as u32,
                    render_api::CLASSIC_SKIN_SIDE as u32,
                ),
            ),
            ("broken.png", vec![1]),
        ] {
            archive
                .start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            archive.write_all(&bytes).unwrap();
        }
        let files = vec![PickedFile {
            name: "fixture.mcpack".into(),
            bytes: archive.finish().unwrap().into_inner(),
        }];
        let mut import = Import::begin(&view, files).unwrap();
        loop {
            match import.step() {
                Err(_) => break,
                Ok(true) => panic!("an invalid pack entry was accepted"),
                Ok(false) => assert_eq!(view.skins.len(), 1, "partial imports stay private"),
            }
        }
        assert_eq!(view.skins.len(), 1);
        assert_eq!(view.active_skin(), Some(fallback.standard_skin()));
    }

    #[test]
    fn invalid_input_and_cancelled_work_keep_the_prior_appearance() {
        let fallback = LocalPlayerSkin::generated_default("cancel fixture");
        let view = load(&fallback);
        let before = view.clone();
        let mut import = Import::begin(
            &view,
            vec![PickedFile {
                name: "bad.png".into(),
                bytes: vec![1],
            }],
        )
        .unwrap();
        assert!(!import.step().unwrap());
        assert!(import.step().is_err());
        drop(import);
        assert_eq!(view, before);
        let mut caped = view.clone();
        let (cape_width, cape_height) = render_api::CAPE_DIMENSIONS[0];
        import_cape(
            &mut caped,
            vec![PickedFile {
                name: "Cape.png".into(),
                bytes: png(cape_width, cape_height),
            }],
        )
        .unwrap();
        assert_eq!(caped.active_skin().unwrap().rgba8, fallback.rgba8);
        assert!(caped.active_skin().unwrap().cape.is_some());
        assert_eq!(valid_name("  Fixture  ").unwrap(), "Fixture");
        assert!(geometry(&view, SkinModel::Slim).is_err());
    }
}
