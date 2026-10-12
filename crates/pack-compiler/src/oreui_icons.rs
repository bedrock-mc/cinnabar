use std::{
    collections::HashSet,
    fs,
    io::Cursor,
    path::{Component, Path},
};

use ::image::{ImageFormat, ImageReader};
use assets::{
    canonical_source_manifest_sha256,
    oreui_icons::{MAX_OREUI_ICON_SIDE, OreUiIcon, OreUiIconFrame, encode_oreui_icons},
};
use serde::Deserialize;
use sha2::{Digest, Sha256};

const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const MAX_SOURCE_BYTES: u64 = 4 * 1024 * 1024;
const REGENERATE: &str = "python3 tools/oreui_icons/build.py";

/// The generator records itself beside these fields; the compiler needs only the images.
#[derive(Deserialize)]
struct Manifest {
    schema: u32,
    images: Vec<Image>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Image {
    key: String,
    frames: Vec<Frame>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Frame {
    file: String,
    sha256: String,
    millis: Option<u32>,
}

fn read_bounded(path: &Path, max: u64) -> Result<Vec<u8>, String> {
    let length = fs::metadata(path)
        .map_err(|error| format!("{}: {error}", path.display()))?
        .len();
    if length > max {
        return Err(format!("{} exceeds {max} bytes", path.display()));
    }
    fs::read(path).map_err(|error| format!("{}: {error}", path.display()))
}

/// Decodes one source PNG after checking it against its manifest digest.
fn read_frame(root: &Path, frame: &Frame) -> Result<(u32, u32, Box<[u8]>), String> {
    let relative = Path::new(&frame.file);
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(format!("invalid OreUI icon source path: {}", frame.file));
    }
    let path = root.join(relative);
    let bytes = read_bounded(&path, MAX_SOURCE_BYTES)?;
    if format!("{:x}", Sha256::digest(&bytes)) != frame.sha256 {
        return Err(format!(
            "{} does not match its manifest digest; regenerate with {REGENERATE}",
            path.display()
        ));
    }
    let decoded = ImageReader::with_format(Cursor::new(bytes), ImageFormat::Png)
        .decode()
        .map_err(|error| format!("{}: {error}", path.display()))?
        .into_rgba8();
    let (width, height) = decoded.dimensions();
    if width > MAX_OREUI_ICON_SIDE || height > MAX_OREUI_ICON_SIDE {
        return Err(format!("{} exceeds the OreUI icon side", path.display()));
    }
    Ok((width, height, decoded.into_raw().into_boxed_slice()))
}

/// Compiles the shipped OreUI artwork named by `manifest`, whose frame files resolve beside it,
/// into the MCBEOUI1 carrier at `out`.
pub fn compile_oreui_icons_to_file(manifest: &Path, out: &Path) -> Result<usize, String> {
    let source = read_bounded(manifest, MAX_MANIFEST_BYTES)?;
    let parsed: Manifest = serde_json::from_slice(&source)
        .map_err(|error| format!("{}: {error}", manifest.display()))?;
    if parsed.schema != 1 {
        return Err(format!("{}: unsupported schema", manifest.display()));
    }
    let root = manifest.parent().unwrap_or(Path::new("."));
    let mut keys = HashSet::new();
    let mut icons = Vec::with_capacity(parsed.images.len());
    for image in &parsed.images {
        if !keys.insert(image.key.as_str()) {
            return Err(format!("duplicate OreUI icon key {}", image.key));
        }
        let animated = image.frames.len() > 1;
        let mut size = None;
        let mut frames = Vec::with_capacity(image.frames.len());
        for frame in &image.frames {
            let (width, height, rgba8) = read_frame(root, frame)?;
            if *size.get_or_insert((width, height)) != (width, height) {
                return Err(format!("{}: frames differ in size", image.key));
            }
            let millis = match (animated, frame.millis) {
                (true, Some(millis)) if millis > 0 => millis,
                (false, None) => 0,
                _ => {
                    return Err(format!(
                        "{}: animation frames need positive millis; stills have none",
                        image.key
                    ));
                }
            };
            frames.push(OreUiIconFrame { millis, rgba8 });
        }
        let (width, height) = size.ok_or_else(|| format!("{} has no frames", image.key))?;
        icons.push(OreUiIcon {
            key: image.key.clone(),
            width,
            height,
            frames,
        });
    }
    let blob = encode_oreui_icons(canonical_source_manifest_sha256(&source), &icons)
        .map_err(|error| error.to_string())?;
    let temporary = out.with_extension("tmp");
    fs::write(&temporary, &blob).map_err(|error| format!("{}: {error}", temporary.display()))?;
    fs::rename(&temporary, out).map_err(|error| format!("{}: {error}", out.display()))?;
    Ok(icons.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use assets::oreui_icons::decode_oreui_icons;

    fn png(dir: &Path, name: &str, width: u32, height: u32) -> String {
        let image = ::image::RgbaImage::from_pixel(width, height, ::image::Rgba([9, 8, 7, 255]));
        let path = dir.join(name);
        image.save(&path).unwrap();
        format!("{:x}", Sha256::digest(fs::read(path).unwrap()))
    }

    #[test]
    fn compiles_stills_and_animations_and_rejects_stale_sources() {
        let dir = tempfile::tempdir().unwrap();
        let still = png(dir.path(), "still.png", 3, 2);
        let a = png(dir.path(), "a.png", 2, 2);
        let b = png(dir.path(), "b.png", 2, 2);
        let manifest = dir.path().join("source.json");
        let text = format!(
            r#"{{"schema":1,"generator":"g","images":[
            {{"key":"assets/s.png","frames":[{{"file":"still.png","sha256":"{still}"}}]}},
            {{"key":"assets/a.gif","frames":[{{"file":"a.png","sha256":"{a}","millis":100}},
                {{"file":"b.png","sha256":"{b}","millis":50}}]}}]}}"#
        );
        fs::write(&manifest, &text).unwrap();
        let out = dir.path().join("out.mcbeoui");
        assert_eq!(compile_oreui_icons_to_file(&manifest, &out).unwrap(), 2);
        let icons = decode_oreui_icons(
            &fs::read(&out).unwrap(),
            canonical_source_manifest_sha256(text.as_bytes()),
        )
        .unwrap();
        assert_eq!((icons[0].width, icons[0].height), (3, 2));
        assert_eq!(
            icons[1].frames.iter().map(|f| f.millis).collect::<Vec<_>>(),
            [100, 50]
        );
        png(dir.path(), "a.png", 2, 3);
        let error = compile_oreui_icons_to_file(&manifest, &out).unwrap_err();
        assert!(error.contains(REGENERATE), "{error}");
    }

    #[test]
    fn rejects_paths_outside_the_source_directory() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = dir.path().join("source.json");
        fs::write(
            &manifest,
            r#"{"schema":1,"generator":"g","images":[
            {"key":"k","frames":[{"file":"../x.png","sha256":"00"}]}]}"#,
        )
        .unwrap();
        let error = compile_oreui_icons_to_file(&manifest, &dir.path().join("o")).unwrap_err();
        assert!(error.contains("invalid OreUI icon source path"), "{error}");
    }
}
