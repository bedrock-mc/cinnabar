//! Source authentication and a resource-only local carrier.

use super::{
    JavaBiome, JavaEnvironment, JavaTexture, JavaWorldAssets, MAX_CARRIER_BYTES, SOURCE_MANIFEST,
    timeline::Timeline,
};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    io::{Cursor, Read, Write},
};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;
const MAGIC: &[u8; 8] = b"MCJWV001";
const MAX_ENTRY_BYTES: u64 = 1024 * 1024;
const MAX_RESOURCE_BYTES: usize = 16 * 1024 * 1024;
const DIMENSIONS: [&str; 3] = ["overworld", "the_nether", "the_end"];

/// The exact official download used by every install, independent of future releases.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JavaWorldSource {
    pub version: Box<str>,
    pub url: Box<str>,
    pub bytes: usize,
    pub sha256: Box<str>,
}

impl JavaWorldSource {
    /// Reads the shared source pin embedded with the asset loader.
    pub fn pinned() -> Result<Self> {
        Ok(serde_json::from_str(SOURCE_MANIFEST)?)
    }

    /// Verifies length and digest before any client archive entry is interpreted.
    pub fn verify(&self, bytes: &[u8]) -> Result<()> {
        let actual = format!("{:x}", Sha256::digest(bytes));
        if bytes.len() != self.bytes || actual != self.sha256.as_ref() {
            return Err("Java world source does not match its pinned length and SHA-256".into());
        }
        Ok(())
    }
}

/// Builds a deterministic visual-resource carrier without retaining any game classes.
pub fn compile_java_world(client: &[u8]) -> Result<Vec<u8>> {
    JavaWorldSource::pinned()?.verify(client)?;
    let mut source = ZipArchive::new(Cursor::new(client))?;
    let mut names: Vec<_> = source
        .file_names()
        .filter(|name| selected(name))
        .map(str::to_owned)
        .collect();
    names.sort_unstable();
    let mut destination = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut total = 0;
    for name in names {
        let mut bytes = read_entry(&mut source, &name)?;
        if name.ends_with(".json") {
            let mut data: Value = serde_json::from_slice(&bytes)?;
            if name.starts_with("data/minecraft/worldgen/biome/") {
                data.as_object_mut()
                    .ok_or("invalid Java biome")?
                    .retain(|key, _| {
                        matches!(
                            key.as_str(),
                            "attributes"
                                | "effects"
                                | "temperature"
                                | "downfall"
                                | "has_precipitation"
                        )
                    });
            }
            if let Some(attributes) = data.get_mut("attributes").and_then(Value::as_object_mut) {
                attributes.retain(|key, _| key.starts_with("minecraft:visual/"));
            }
            if let Some(tracks) = data.get_mut("tracks").and_then(Value::as_object_mut) {
                tracks.retain(|key, _| key.starts_with("minecraft:visual/"));
            }
            bytes = serde_json::to_vec(&data)?;
        }
        total += bytes.len();
        if total > MAX_RESOURCE_BYTES {
            return Err("Java visual resources exceed their bound".into());
        }
        destination.start_file(name, options)?;
        destination.write_all(&bytes)?;
    }
    let payload = destination.finish()?.into_inner();
    let mut encoded = Vec::with_capacity(payload.len() + 72);
    encoded.extend_from_slice(MAGIC);
    encoded.extend_from_slice(&Sha256::digest(SOURCE_MANIFEST.as_bytes()));
    encoded.extend_from_slice(&payload);
    let digest = Sha256::digest(&encoded);
    encoded.extend_from_slice(&digest);
    decode_java_world(&encoded)?;
    Ok(encoded)
}

/// Authenticates a local carrier and decodes only bounded, renderer-owned resources.
pub fn decode_java_world(bytes: &[u8]) -> Result<JavaWorldAssets> {
    if !(72..=MAX_CARRIER_BYTES).contains(&bytes.len()) || &bytes[..8] != MAGIC {
        return Err("invalid Java world carrier header or length".into());
    }
    let (body, digest) = bytes.split_at(bytes.len() - 32);
    if Sha256::digest(body).as_slice() != digest
        || body[8..40] != Sha256::digest(SOURCE_MANIFEST.as_bytes())[..]
    {
        return Err("Java world carrier digest or source mismatch".into());
    }
    let mut archive = ZipArchive::new(Cursor::new(&body[40..]))?;
    if archive.len() > 512 {
        return Err("too many Java visual resources".into());
    }
    let names: Vec<_> = archive.file_names().map(str::to_owned).collect();
    let mut seen = BTreeSet::new();
    let mut total = 0;
    let mut textures = BTreeMap::new();
    let mut jsons = BTreeMap::new();
    for name in names {
        if !selected(&name) || !seen.insert(name.clone()) {
            return Err("invalid or duplicate Java visual resource".into());
        }
        let contents = read_entry(&mut archive, &name)?;
        total += contents.len();
        if total > MAX_RESOURCE_BYTES {
            return Err("decoded Java visual resources exceed their bound".into());
        }
        if name.ends_with(".png") {
            textures.insert(name.into_boxed_str(), decode_texture(&contents)?);
        } else {
            jsons.insert(name, serde_json::from_slice::<Value>(&contents)?);
        }
    }
    let mut dimensions = [JavaEnvironment::default(); 3];
    for (index, name) in DIMENSIONS.into_iter().enumerate() {
        let path = format!("data/minecraft/dimension_type/{name}.json");
        let data = jsons
            .get(&path)
            .ok_or("missing Java dimension visual resource")?;
        dimensions[index] = JavaEnvironment::parse(&data["attributes"])?;
    }
    let day = jsons
        .get("data/minecraft/timeline/day.json")
        .ok_or("missing Java visual day timeline")?;
    let timelines = vec![Timeline::parse(day)?];
    let atmosphere = super::textures::atmosphere_textures(&textures, &jsons)?;
    let water = super::water::decode(&textures, &jsons)?;
    let mut biomes = BTreeMap::new();
    for (path, data) in jsons {
        let Some(name) = path
            .strip_prefix("data/minecraft/worldgen/biome/")
            .and_then(|name| name.strip_suffix(".json"))
        else {
            continue;
        };
        biomes.insert(
            format!("minecraft:{name}").into_boxed_str(),
            parse_biome(&data)?,
        );
    }
    if biomes.is_empty() || !biomes.contains_key("minecraft:plains") {
        return Err("missing Java biome visual resources".into());
    }
    for required in [
        "environment/clouds",
        "environment/celestial/sun",
        "colormap/grass",
        "colormap/foliage",
    ] {
        if !textures.contains_key(format!("assets/minecraft/textures/{required}.png").as_str()) {
            return Err(format!("missing Java texture {required}").into());
        }
    }
    Ok(JavaWorldAssets {
        source: JavaWorldSource::pinned()?,
        textures,
        biomes,
        dimensions,
        timelines,
        atmosphere,
        water,
    })
}

/// Keeps only data and art used by world presentation; classes and executable resources are excluded.
fn selected(name: &str) -> bool {
    if name.contains("..") || name.contains('\\') {
        return false;
    }
    super::WATER_TEXTURE_PATHS
        .iter()
        .any(|path| name.strip_suffix(".mcmeta") == Some(*path))
        || (name.ends_with(".json")
            && (name.starts_with("data/minecraft/worldgen/biome/")
                || DIMENSIONS.iter().any(|dimension| {
                    name == format!("data/minecraft/dimension_type/{dimension}.json")
                })
                || matches!(
                    name,
                    "data/minecraft/timeline/day.json" | "data/minecraft/timeline/moon.json"
                )))
        || (name.ends_with(".png")
            && (name.starts_with("assets/minecraft/textures/environment/")
                || name.starts_with("assets/minecraft/textures/colormap/")
                || ["water_", "lava_", "fire_"].iter().any(|prefix| {
                    name.strip_prefix("assets/minecraft/textures/block/")
                        .is_some_and(|name| name.starts_with(prefix))
                })))
}

/// Reads one compressed entry without trusting its declared size to bound allocation.
fn read_entry<R: Read + std::io::Seek>(archive: &mut ZipArchive<R>, name: &str) -> Result<Vec<u8>> {
    let entry = archive.by_name(name)?;
    if entry.size() > MAX_ENTRY_BYTES {
        return Err(format!("Java resource {name} is too large").into());
    }
    let mut contents = Vec::new();
    entry.take(MAX_ENTRY_BYTES + 1).read_to_end(&mut contents)?;
    if contents.len() as u64 > MAX_ENTRY_BYTES {
        return Err(format!("Java resource {name} exceeds its decoded bound").into());
    }
    Ok(contents)
}

/// Converts a finite PNG into bounded RGBA8 pixels once, during resource loading.
fn decode_texture(bytes: &[u8]) -> Result<JavaTexture> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info()?;
    let (width, height) = reader.info().size();
    if width == 0 || height == 0 || width > 1024 || height > 4096 {
        return Err("Java texture dimensions exceed bounds".into());
    }
    let size = reader
        .output_buffer_size()
        .filter(|size| *size <= MAX_RESOURCE_BYTES)
        .ok_or("Java texture is too large")?;
    let mut pixels = vec![0; size];
    let output = reader.next_frame(&mut pixels)?;
    let channels = output.color_type.samples();
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for pixel in pixels[..output.buffer_size()].chunks_exact(channels) {
        match output.color_type {
            png::ColorType::Rgba => rgba.extend_from_slice(pixel),
            png::ColorType::Rgb => rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]),
            png::ColorType::Grayscale => {
                rgba.extend_from_slice(&[pixel[0], pixel[0], pixel[0], 255])
            }
            png::ColorType::GrayscaleAlpha => {
                rgba.extend_from_slice(&[pixel[0], pixel[0], pixel[0], pixel[1]])
            }
            png::ColorType::Indexed => return Err("unexpanded indexed Java texture".into()),
        }
    }
    Ok(JavaTexture {
        width,
        height,
        rgba: rgba.into_boxed_slice(),
    })
}

/// Reads authored biome colours and climate without carrying gameplay definitions into the client.
fn parse_biome(data: &Value) -> Result<JavaBiome> {
    let colour = |name: &str| -> Result<Option<u32>> {
        data["effects"]
            .get(name)
            .map(|value| {
                let value = value
                    .as_str()
                    .and_then(|value| value.strip_prefix('#'))
                    .ok_or("invalid Java biome colour")?;
                Ok(u32::from_str_radix(value, 16)?)
            })
            .transpose()
    };
    let finite = |name: &str| -> Result<f32> {
        Ok(data[name]
            .as_f64()
            .filter(|value| value.is_finite() && value.abs() <= 100.0)
            .ok_or("invalid Java biome climate")? as f32)
    };
    Ok(JavaBiome {
        environment: JavaEnvironment::parse(&data["attributes"])?,
        temperature: finite("temperature")?,
        downfall: finite("downfall")?,
        water: colour("water_color")?.ok_or("missing Java biome water colour")?,
        grass: colour("grass_color")?,
        foliage: colour("foliage_color")?,
        dry_foliage: colour("dry_foliage_color")?,
        grass_modifier: data["effects"]["grass_color_modifier"]
            .as_str()
            .map(Into::into),
        precipitation: data["has_precipitation"]
            .as_bool()
            .ok_or("missing Java biome precipitation")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_and_oversized_carriers_are_rejected() {
        for bytes in [Vec::new(), vec![0; 72], vec![0; MAX_CARRIER_BYTES + 1]] {
            assert!(decode_java_world(&bytes).is_err());
        }
        assert!(
            JavaWorldSource::pinned()
                .unwrap()
                .verify(b"untrusted archive")
                .is_err()
        );
    }

    #[test]
    fn installed_reference_compiles_and_decodes_its_visual_resources() {
        let Some(path) = std::env::var_os("CINNABAR_JAVA_CLIENT_JAR") else {
            eprintln!(
                "missing fixture: set CINNABAR_JAVA_CLIENT_JAR to the pinned Java client archive"
            );
            return;
        };
        let source = std::fs::read(path).expect("read configured Java client fixture");
        let mut carrier =
            compile_java_world(&source).expect("compile authenticated visual resources");
        let decoded = decode_java_world(&carrier).expect("decode visual resources");
        assert!(decoded.biome_colours("minecraft:plains").is_some());
        assert!(decoded.water.iter().all(|water| water.frame_count() > 1));
        assert_ne!(
            decoded.environment(0, Some("minecraft:plains"), 6000.0),
            decoded.environment(0, Some("minecraft:plains"), 18000.0),
        );
        carrier[40] ^= 1;
        assert!(decode_java_world(&carrier).is_err());
    }
}
