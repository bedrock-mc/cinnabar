//! Packs opened from a zip translate exactly like the same pack as a folder.

use crate::{fixture_path, open_fixture};
use shaderpack::{PackOrigin, PackTranslation, ShaderPack, TranslateConfig};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

/// A fresh directory under the system temp dir, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> TempDir {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("shaderpack-it-{label}-{}-{n}", std::process::id()));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Every file under `dir`, as (`/`-separated path relative to `dir`, absolute path), sorted.
fn files_under(dir: &Path) -> Vec<(String, PathBuf)> {
    let mut out = vec![];
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let rel = path
                    .strip_prefix(dir)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push((rel, path));
            }
        }
    }
    out.sort();
    out
}

/// Writes a stored zip of the pack folder `pack` with its files under `<top>/`.
fn zip_pack(pack: &Path, top: &str, archive: &Path) {
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(std::fs::File::create(archive).unwrap());
    zip.add_directory(format!("{top}/"), options).unwrap();
    for (rel, path) in files_under(pack) {
        zip.start_file(format!("{top}/{rel}"), options).unwrap();
        zip.write_all(&std::fs::read(path).unwrap()).unwrap();
    }
    zip.finish().unwrap();
}

/// Asserts two translations of the same pack agree program by program.
fn assert_same_translation(folder: &PackTranslation, zipped: &PackTranslation) {
    assert_eq!(folder.properties, zipped.properties);
    assert_eq!(folder.buffers, zipped.buffers);
    assert_eq!(folder.programs.len(), zipped.programs.len());
    for (a, b) in folder.programs.iter().zip(&zipped.programs) {
        let name = &a.program.name;
        assert_eq!(a.program, b.program, "{name}");
        assert_eq!((a.enabled, &a.forced), (b.enabled, &b.forced), "{name}");
        assert_eq!(a.reflection, b.reflection, "{name}");
        assert_eq!(a.diagnostics, b.diagnostics, "{name}");
        assert_eq!(a.draw_buffers, b.draw_buffers, "{name}");
        assert_eq!(a.consts, b.consts, "{name}");
        assert_eq!(a.stages.len(), b.stages.len(), "{name}");
        for (sa, sb) in a.stages.iter().zip(&b.stages) {
            assert_eq!((sa.stage, &sa.path), (sb.stage, &sb.path), "{name}");
            match (&sa.result, &sb.result) {
                (Ok(x), Ok(y)) => assert_eq!(x.glsl, y.glsl, "{name} {}", sa.path),
                (Err(x), Err(y)) => assert_eq!(x, y, "{name} {}", sa.path),
                _ => panic!("{name} {}: folder and zip disagree on success", sa.path),
            }
        }
    }
}

fn check_zip_matches_folder(name: &str, config: &TranslateConfig) {
    let Some(folder_pack) = open_fixture(name) else {
        return;
    };
    let tmp = TempDir::new(name);
    let archive = tmp.0.join(format!("{name}.zip"));
    let top = format!("{name} pack v1");
    zip_pack(&fixture_path(name), &top, &archive);

    let zipped = ShaderPack::open(&archive).unwrap();
    assert_eq!(
        zipped.source().origin(),
        &PackOrigin::Zip {
            archive: archive.canonicalize().unwrap(),
            prefix: format!("{top}/shaders/"),
        }
    );
    let folder_files: Vec<(&str, &str)> = folder_pack.source().files().collect();
    let zip_files: Vec<(&str, &str)> = zipped.source().files().collect();
    assert_eq!(folder_files, zip_files);
    assert_eq!(folder_pack.options(), zipped.options());
    assert_same_translation(&folder_pack.translate(config), &zipped.translate(config));
    drop(tmp);
}

#[test]
fn zipped_pack_with_includes_and_errors_matches_the_folder() {
    check_zip_matches_folder("includes", &TranslateConfig::default());
}

#[test]
fn zipped_pack_with_options_matches_the_folder_under_force_mode() {
    let config = TranslateConfig {
        force_disabled: true,
        ..TranslateConfig::default()
    };
    check_zip_matches_folder("options", &config);
}
