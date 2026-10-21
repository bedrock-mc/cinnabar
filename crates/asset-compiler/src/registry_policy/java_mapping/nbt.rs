//! Reads the generator's gzip-compressed, big-endian network exports: `networkTags.nbt`, a root
//! compound of registries each mapping tag ids to integer arrays of registry ids, and
//! `networkCodec.nbt`, a root compound of registries each holding `{type, value: [{name, id,
//! element}]}`, of which the biome and dimension type registries are read.

use super::Result;
use assets::facts::java_mapping::JavaSkyColours;
use flate2::read::GzDecoder;
use std::io::Read;

pub(super) const BLOCK_REGISTRY: &str = "minecraft:block";
pub(super) const BIOME_REGISTRY: &str = "minecraft:worldgen/biome";
pub(super) const DIMENSION_TYPE_REGISTRY: &str = "minecraft:dimension_type";
/// The environment attributes kept in [`JavaSkyColours`], in field order.
const COLOUR_ATTRIBUTES: [&str; 3] = [
    "minecraft:visual/sky_color",
    "minecraft:visual/fog_color",
    "minecraft:visual/water_fog_color",
];
const MAX_INFLATED_BYTES: u64 = 16 * 1024 * 1024;
const MAX_DEPTH: usize = 32;

const END: u8 = 0;
const BYTE: u8 = 1;
const SHORT: u8 = 2;
const INT: u8 = 3;
const LONG: u8 = 4;
const FLOAT: u8 = 5;
const DOUBLE: u8 = 6;
const BYTE_ARRAY: u8 = 7;
const STRING: u8 = 8;
const LIST: u8 = 9;
const COMPOUND: u8 = 10;
const INT_ARRAY: u8 = 11;
const LONG_ARRAY: u8 = 12;

/// One `minecraft:worldgen/biome` entry of the network codec.
#[derive(Debug, PartialEq)]
pub(super) struct CodecBiome {
    pub name: String,
    pub id: i32,
    pub temperature: f32,
    pub downfall: f32,
    pub has_precipitation: bool,
    /// `temperature_modifier` is `frozen` (absent or `none` otherwise).
    pub frozen: bool,
    /// The colour attributes the biome sets.
    pub colours: JavaSkyColours,
}

/// One `minecraft:dimension_type` entry of the network codec: its id and colour attributes.
#[derive(Debug, PartialEq)]
pub(super) struct CodecDimension {
    pub name: String,
    pub colours: JavaSkyColours,
}

/// Every tag of `registry` in a network tag export with its member registry ids, in file order.
pub(super) fn tags(compressed: &[u8], registry: &str) -> Result<Vec<(String, Vec<i32>)>> {
    registry_entry(compressed, registry, |reader| reader.int_lists())
}

/// The biome registry of a network codec export, in file order.
pub(super) fn biomes(compressed: &[u8]) -> Result<Vec<CodecBiome>> {
    registry_entry(compressed, BIOME_REGISTRY, |reader| {
        reader.registry(BIOME_REGISTRY, Reader::biome_entry)
    })
}

/// The dimension type registry of a network codec export, in file order.
pub(super) fn dimensions(compressed: &[u8]) -> Result<Vec<CodecDimension>> {
    registry_entry(compressed, DIMENSION_TYPE_REGISTRY, |reader| {
        reader.registry(DIMENSION_TYPE_REGISTRY, Reader::dimension_entry)
    })
}

/// Inflates an export and reads the compound value of `registry` in its root compound with
/// `read`, skipping every other registry; errors when the registry is missing or repeated, the
/// export is malformed, or bytes trail the root.
fn registry_entry<T>(
    compressed: &[u8],
    registry: &str,
    read: impl Fn(&mut Reader) -> Result<T>,
) -> Result<T> {
    let mut bytes = Vec::new();
    GzDecoder::new(compressed)
        .take(MAX_INFLATED_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_INFLATED_BYTES {
        return Err("network export exceeds its size limit".into());
    }
    let mut reader = Reader { bytes: &bytes };
    if reader.u8()? != COMPOUND {
        return Err("network export has no root compound".into());
    }
    reader.string()?;
    let mut found = None;
    loop {
        let kind = reader.u8()?;
        if kind == END {
            break;
        }
        let name = reader.string()?;
        if kind != COMPOUND || name != registry {
            reader.skip(kind, 0)?;
        } else if found.is_none() {
            found = Some(read(&mut reader)?);
        } else {
            return Err(format!("network export repeats {registry}").into());
        }
    }
    if !reader.bytes.is_empty() {
        return Err("trailing bytes after the network export".into());
    }
    found.ok_or_else(|| format!("network export has no {registry} registry").into())
}

struct Reader<'a> {
    bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        if self.bytes.len() < count {
            return Err("truncated network export".into());
        }
        let (head, tail) = self.bytes.split_at(count);
        self.bytes = tail;
        Ok(head)
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_be_bytes(self.take(4)?.try_into()?))
    }

    fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_be_bytes(self.take(4)?.try_into()?))
    }

    fn length(&mut self) -> Result<usize> {
        usize::try_from(self.i32()?).map_err(|_| "negative NBT length".into())
    }

    fn string(&mut self) -> Result<String> {
        let length = u16::from_be_bytes(self.take(2)?.try_into()?);
        Ok(std::str::from_utf8(self.take(usize::from(length))?)?.to_owned())
    }

    /// A compound whose values are int arrays (or lists of ints).
    fn int_lists(&mut self) -> Result<Vec<(String, Vec<i32>)>> {
        let mut entries = Vec::new();
        loop {
            let kind = self.u8()?;
            if kind == END {
                return Ok(entries);
            }
            let name = self.string()?;
            let values = match kind {
                INT_ARRAY => (0..self.length()?)
                    .map(|_| self.i32())
                    .collect::<Result<_>>()?,
                LIST => {
                    let element = self.u8()?;
                    let count = self.length()?;
                    if element != INT && count > 0 {
                        return Err(format!("tag {name} is not a list of ints").into());
                    }
                    (0..count).map(|_| self.i32()).collect::<Result<_>>()?
                }
                _ => return Err(format!("tag {name} is not an int array").into()),
            };
            entries.push((name, values));
        }
    }

    /// A codec registry compound: `type` names the registry and `value` lists its entries,
    /// each read by `entry`.
    fn registry<T>(
        &mut self,
        registry: &str,
        entry: impl Fn(&mut Self) -> Result<T>,
    ) -> Result<Vec<T>> {
        let mut kind_name = None;
        let mut entries = None;
        loop {
            let kind = self.u8()?;
            if kind == END {
                break;
            }
            match (kind, self.string()?.as_str()) {
                (STRING, "type") => kind_name = Some(self.string()?),
                (LIST, "value") => {
                    let element = self.u8()?;
                    let count = self.length()?;
                    if element != COMPOUND && count > 0 {
                        return Err(format!("{registry} value is not a list of compounds").into());
                    }
                    entries = Some(
                        (0..count)
                            .map(|_| entry(self))
                            .collect::<Result<Vec<_>>>()?,
                    );
                }
                (_, field) => return Err(format!("unexpected {registry} field {field}").into()),
            }
        }
        if kind_name.as_deref() != Some(registry) {
            return Err(format!("registry type is not {registry}").into());
        }
        entries.ok_or_else(|| format!("{registry} has no value list").into())
    }

    /// An `attributes` compound's [`COLOUR_ATTRIBUTES`] (`"#rrggbb"` strings); every other
    /// attribute is skipped.
    fn colour_attributes(&mut self) -> Result<JavaSkyColours> {
        let mut colours = [None; 3];
        loop {
            let kind = self.u8()?;
            if kind == END {
                break;
            }
            let name = self.string()?;
            let Some(slot) = COLOUR_ATTRIBUTES.iter().position(|&known| known == name) else {
                self.skip(kind, 2)?;
                continue;
            };
            if kind != STRING {
                return Err(format!("attribute {name} is not a colour string").into());
            }
            let text = self.string()?;
            let colour = text
                .strip_prefix('#')
                .filter(|hex| hex.len() == 6)
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                .ok_or_else(|| format!("attribute {name} is {text}, not #rrggbb"))?;
            colours[slot] = Some(colour);
        }
        let [sky, fog, water_fog] = colours;
        Ok(JavaSkyColours {
            sky,
            fog,
            water_fog,
        })
    }

    /// One dimension type entry `{name, id, element}`: its name and the colour attributes of
    /// its element, every other field skipped.
    fn dimension_entry(&mut self) -> Result<CodecDimension> {
        let (mut name, mut colours) = (None, JavaSkyColours::default());
        loop {
            let kind = self.u8()?;
            if kind == END {
                break;
            }
            match (kind, self.string()?.as_str()) {
                (STRING, "name") => name = Some(self.string()?),
                (COMPOUND, "element") => loop {
                    let kind = self.u8()?;
                    if kind == END {
                        break;
                    }
                    match (kind, self.string()?.as_str()) {
                        (COMPOUND, "attributes") => colours = self.colour_attributes()?,
                        _ => self.skip(kind, 2)?,
                    }
                },
                (kind, _) => self.skip(kind, 1)?,
            }
        }
        Ok(CodecDimension {
            name: name.ok_or("dimension type entry without a name")?,
            colours,
        })
    }

    /// One registry entry `{name, id, element}` with the climate fields and colour attributes
    /// of its element; the element's `effects` and other attributes are skipped and any other
    /// field is rejected.
    fn biome_entry(&mut self) -> Result<CodecBiome> {
        let (mut name, mut id) = (None, None);
        let (mut temperature, mut downfall, mut precipitation) = (None, None, None);
        let mut frozen = false;
        let mut colours = JavaSkyColours::default();
        loop {
            let kind = self.u8()?;
            if kind == END {
                break;
            }
            match (kind, self.string()?.as_str()) {
                (STRING, "name") => name = Some(self.string()?),
                (INT, "id") => id = Some(self.i32()?),
                (COMPOUND, "element") => loop {
                    let kind = self.u8()?;
                    if kind == END {
                        break;
                    }
                    match (kind, self.string()?.as_str()) {
                        (FLOAT, "temperature") => temperature = Some(self.f32()?),
                        (FLOAT, "downfall") => downfall = Some(self.f32()?),
                        (BYTE, "has_precipitation") => {
                            precipitation = Some(match self.u8()? {
                                0 => false,
                                1 => true,
                                other => {
                                    return Err(format!(
                                        "has_precipitation is {other}, not 0 or 1"
                                    )
                                    .into());
                                }
                            });
                        }
                        (STRING, "temperature_modifier") => {
                            frozen = match self.string()?.as_str() {
                                "none" => false,
                                "frozen" => true,
                                other => {
                                    return Err(
                                        format!("unknown temperature_modifier {other}").into()
                                    );
                                }
                            };
                        }
                        (COMPOUND, "attributes") => colours = self.colour_attributes()?,
                        (COMPOUND, "effects") => self.skip(COMPOUND, 1)?,
                        (_, field) => {
                            return Err(format!("unexpected biome element field {field}").into());
                        }
                    }
                },
                (_, field) => return Err(format!("unexpected biome entry field {field}").into()),
            }
        }
        let name = name.ok_or("biome entry without a name")?;
        let missing = |field: &str| format!("biome {name} has no {field}");
        Ok(CodecBiome {
            id: id.ok_or_else(|| missing("id"))?,
            temperature: temperature.ok_or_else(|| missing("temperature"))?,
            downfall: downfall.ok_or_else(|| missing("downfall"))?,
            has_precipitation: precipitation.ok_or_else(|| missing("has_precipitation"))?,
            frozen,
            colours,
            name,
        })
    }

    fn skip(&mut self, kind: u8, depth: usize) -> Result<()> {
        if depth > MAX_DEPTH {
            return Err("network export nests too deeply".into());
        }
        match kind {
            BYTE => {
                self.take(1)?;
            }
            SHORT => {
                self.take(2)?;
            }
            INT | FLOAT => {
                self.take(4)?;
            }
            LONG | DOUBLE => {
                self.take(8)?;
            }
            BYTE_ARRAY => {
                let count = self.length()?;
                self.take(count)?;
            }
            STRING => {
                self.string()?;
            }
            LIST => {
                let element = self.u8()?;
                for _ in 0..self.length()? {
                    self.skip(element, depth + 1)?;
                }
            }
            COMPOUND => loop {
                let kind = self.u8()?;
                if kind == END {
                    break;
                }
                self.string()?;
                self.skip(kind, depth + 1)?;
            },
            INT_ARRAY => {
                let count = self.length()?;
                self.take(count.checked_mul(4).ok_or("NBT length overflow")?)?;
            }
            LONG_ARRAY => {
                let count = self.length()?;
                self.take(count.checked_mul(8).ok_or("NBT length overflow")?)?;
            }
            _ => return Err(format!("unknown NBT tag type {kind}").into()),
        }
        Ok(())
    }
}
