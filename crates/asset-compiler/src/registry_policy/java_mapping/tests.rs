use super::*;
use flate2::{Compression, write::GzEncoder};
use std::io::Write;

fn root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The manifest's archive cache, or `None` (with the missing-fixture notice) when it or a
/// cached Geyser source file is absent.
fn pinned_archive() -> Option<std::path::PathBuf> {
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root().join(java_mapping::SOURCE_MANIFEST)).unwrap())
            .unwrap();
    let geyser = manifest["geyser"]["files"].as_object().unwrap();
    let path = root().join(manifest["archive"]["cache"].as_str().unwrap());
    let missing = std::iter::once(path.clone())
        .chain(
            geyser
                .values()
                .map(|file| root().join(file["cache"].as_str().unwrap())),
        )
        .find(|path| !path.is_file());
    match missing {
        None => Some(path),
        Some(missing) => {
            eprintln!(
                "missing fixture: {}; skipping Java mapping regeneration (registryc --kind java-mapping downloads it)",
                missing.display()
            );
            None
        }
    }
}

#[test]
fn java_mapping_regenerates_every_byte_from_the_pinned_archive() {
    let Some(archive) = pinned_archive() else {
        return;
    };
    let scratch = tempfile::tempdir().unwrap();
    let out = scratch.path().join(java_mapping::FILENAME);
    let summary = write(&root(), Some(&archive), &out).unwrap();
    assert!(
        summary.mapped_states * 10 >= summary.states * 9,
        "{summary:?}"
    );
    let expected = root().join(java_mapping::SOURCE_PATH);
    assert_eq!(
        std::fs::read(&out).unwrap(),
        std::fs::read(&expected).unwrap()
    );
    assert_eq!(
        std::fs::read(out.with_extension("sha256")).unwrap(),
        std::fs::read(expected.with_extension("sha256")).unwrap()
    );
}

#[test]
fn an_unpinned_archive_cannot_supply_the_mapping() {
    let scratch = tempfile::tempdir().unwrap();
    let archive = scratch.path().join("mappings.zip");
    std::fs::write(&archive, b"not the pinned archive").unwrap();
    let out = scratch.path().join("out.bin");
    assert!(write(&root(), Some(&archive), &out).is_err());
    assert!(!out.exists());
}

#[test]
fn identities_prefer_the_same_name_then_the_first_given() {
    let pairs = [
        ("minecraft:illusioner", "minecraft:evocation_illager"),
        ("minecraft:evoker", "minecraft:evocation_illager"),
        ("minecraft:spectral_arrow", "minecraft:arrow"),
        ("minecraft:arrow", "minecraft:arrow"),
        ("minecraft:cow", "minecraft:cow"),
    ]
    .map(|(java, bedrock)| (java.to_owned(), bedrock.to_owned()));
    let (table, inverse) = identities(pairs.to_vec()).unwrap();
    let java: Vec<&str> = table.iter().map(|(java, _)| java.as_ref()).collect();
    assert_eq!(
        java,
        [
            "minecraft:arrow",
            "minecraft:cow",
            "minecraft:evoker",
            "minecraft:illusioner",
            "minecraft:spectral_arrow"
        ]
    );
    let chosen: Vec<(&str, &str)> = inverse
        .iter()
        .map(|&index| {
            let (java, bedrock) = &table[usize::from(index)];
            (bedrock.as_ref(), java.as_ref())
        })
        .collect();
    assert_eq!(
        chosen,
        [
            ("minecraft:arrow", "minecraft:arrow"),
            ("minecraft:cow", "minecraft:cow"),
            ("minecraft:evocation_illager", "minecraft:illusioner"),
        ]
    );
    let mut duplicate = pairs.to_vec();
    duplicate.push(("minecraft:cow".into(), "minecraft:mooshroom".into()));
    assert!(identities(duplicate).is_err());
}

#[test]
fn the_inverse_prefers_the_same_name_then_a_dry_state_then_text_order() {
    assert_eq!(
        preference(
            "minecraft:air",
            &["minecraft:cave_air", "minecraft:air", "minecraft:void_air"]
        )
        .unwrap(),
        ("minecraft:air".to_owned(), String::new())
    );
    assert_eq!(
        preference(
            "minecraft:oak_stairs",
            &[
                "minecraft:oak_stairs[facing=east,half=top,shape=straight,waterlogged=true]",
                "minecraft:oak_stairs[facing=east,half=top,shape=straight,waterlogged=false]",
                "minecraft:oak_stairs[facing=east,half=top,shape=outer_left,waterlogged=false]",
            ]
        )
        .unwrap(),
        (
            "minecraft:oak_stairs".to_owned(),
            "facing=east,half=top,shape=outer_left,waterlogged=false".to_owned()
        )
    );
    assert_eq!(
        preference("minecraft:flowing_water", &["minecraft:water[level=3]"]).unwrap(),
        ("minecraft:water".to_owned(), "level=3".to_owned())
    );
    assert!(preference("minecraft:stone", &["minecraft:stone[level=1"]).is_err());
}

#[test]
fn registry_states_read_bytes_and_ints_as_integers() {
    assert_eq!(
        bedrock_state(
            r#"{"wall_post_bit":{"type":"byte","value":1},"facing":{"type":"string","value":"up"},"age":{"type":"int","value":7}}"#
        )
        .unwrap(),
        vec![
            ("age".to_owned(), Scalar::Int(7)),
            ("facing".to_owned(), Scalar::Text("up".to_owned())),
            ("wall_post_bit".to_owned(), Scalar::Int(1)),
        ]
    );
    assert!(bedrock_state(r#"{"age":{"type":"float","value":1.5}}"#).is_err());
}

#[test]
fn java_block_registry_order_follows_the_registrations() {
    let source = "public static final Block FIRST = register(new Block(\"first_block\", builder()));\n\
                  public static final Block SECOND = register(new CustomBlock(\"second_block\", builder()\n    .booleanState(LIT)));\n";
    assert_eq!(
        java_block_registry(source).unwrap(),
        ["minecraft:first_block", "minecraft:second_block"]
    );
    assert!(java_block_registry("register(new Block(builder()));").is_err());
}

/// Writes a big-endian NBT name.
fn name(out: &mut Vec<u8>, text: &str) {
    out.extend_from_slice(&(text.len() as u16).to_be_bytes());
    out.extend_from_slice(text.as_bytes());
}

#[test]
fn network_tags_yield_block_tags_and_skip_other_registries() {
    let mut nbt = vec![10];
    name(&mut nbt, "");
    // An item registry with one int-array tag the reader must skip.
    nbt.push(10);
    name(&mut nbt, "minecraft:item");
    nbt.push(11);
    name(&mut nbt, "minecraft:things");
    nbt.extend_from_slice(&1_i32.to_be_bytes());
    nbt.extend_from_slice(&5_i32.to_be_bytes());
    nbt.push(0);
    nbt.push(10);
    name(&mut nbt, "minecraft:block");
    nbt.push(11);
    name(&mut nbt, "minecraft:glowing");
    nbt.extend_from_slice(&2_i32.to_be_bytes());
    nbt.extend_from_slice(&3_i32.to_be_bytes());
    nbt.extend_from_slice(&1_i32.to_be_bytes());
    nbt.push(9);
    name(&mut nbt, "minecraft:empty");
    nbt.push(0);
    nbt.extend_from_slice(&0_i32.to_be_bytes());
    nbt.push(0);
    nbt.push(0);
    assert_eq!(
        nbt::tags(&gzip(&nbt), nbt::BLOCK_REGISTRY).unwrap(),
        vec![
            ("minecraft:glowing".to_owned(), vec![3, 1]),
            ("minecraft:empty".to_owned(), vec![]),
        ]
    );
    assert!(nbt::tags(&gzip(&nbt[..nbt.len() - 3]), nbt::BLOCK_REGISTRY).is_err());
    assert!(nbt::tags(&gzip(&nbt), nbt::BIOME_REGISTRY).is_err());
}

fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}

/// Writes one codec biome entry `{name, id, element}`; `modifier` adds `temperature_modifier`.
fn codec_biome(out: &mut Vec<u8>, biome: &str, id: i32, climate: (f32, f32, u8), modifier: &str) {
    out.push(8);
    name(out, "name");
    name(out, biome);
    out.push(3);
    name(out, "id");
    out.extend_from_slice(&id.to_be_bytes());
    out.push(10);
    name(out, "element");
    out.push(5);
    name(out, "temperature");
    out.extend_from_slice(&climate.0.to_be_bytes());
    out.push(5);
    name(out, "downfall");
    out.extend_from_slice(&climate.1.to_be_bytes());
    out.push(1);
    name(out, "has_precipitation");
    out.push(climate.2);
    if !modifier.is_empty() {
        out.push(8);
        name(out, "temperature_modifier");
        name(out, modifier);
    }
    attributes(out, &[("minecraft:visual/sky_color", "#78a7ff")]);
    out.push(10);
    name(out, "effects");
    out.push(8);
    name(out, "water_color");
    name(out, "#3f76e4");
    out.push(0);
    out.push(0);
    out.push(0);
}

/// Writes an `attributes` compound of string `colours` and one skipped compound attribute.
fn attributes(out: &mut Vec<u8>, colours: &[(&str, &str)]) {
    out.push(10);
    name(out, "attributes");
    out.push(10);
    name(out, "minecraft:audio/background_music");
    out.push(0);
    for (attribute, colour) in colours {
        out.push(8);
        name(out, attribute);
        name(out, colour);
    }
    out.push(0);
}

/// A test codec entry: name, registry id, `(temperature, downfall, has_precipitation)` and
/// temperature modifier (empty for none).
type TestBiome<'a> = (&'a str, i32, (f32, f32, u8), &'a str);

/// A codec export with a skipped registry, then a biome registry of `entries`.
fn codec(entries: &[TestBiome]) -> Vec<u8> {
    let mut nbt = vec![10];
    name(&mut nbt, "");
    nbt.push(10);
    name(&mut nbt, "minecraft:chat_type");
    nbt.push(8);
    name(&mut nbt, "type");
    name(&mut nbt, "minecraft:chat_type");
    nbt.push(0);
    nbt.push(10);
    name(&mut nbt, nbt::BIOME_REGISTRY);
    nbt.push(8);
    name(&mut nbt, "type");
    name(&mut nbt, nbt::BIOME_REGISTRY);
    nbt.push(9);
    name(&mut nbt, "value");
    nbt.push(10);
    nbt.extend_from_slice(&(entries.len() as i32).to_be_bytes());
    for &(biome, id, climate, modifier) in entries {
        codec_biome(&mut nbt, biome, id, climate, modifier);
    }
    nbt.push(0);
    nbt.push(10);
    name(&mut nbt, nbt::DIMENSION_TYPE_REGISTRY);
    nbt.push(8);
    name(&mut nbt, "type");
    name(&mut nbt, nbt::DIMENSION_TYPE_REGISTRY);
    nbt.push(9);
    name(&mut nbt, "value");
    nbt.push(10);
    nbt.extend_from_slice(&1i32.to_be_bytes());
    nbt.push(8);
    name(&mut nbt, "name");
    name(&mut nbt, "minecraft:the_end");
    nbt.push(3);
    name(&mut nbt, "id");
    nbt.extend_from_slice(&0i32.to_be_bytes());
    nbt.push(10);
    name(&mut nbt, "element");
    nbt.push(1);
    name(&mut nbt, "has_skylight");
    nbt.push(0);
    attributes(
        &mut nbt,
        &[
            ("minecraft:visual/fog_color", "#181318"),
            ("minecraft:visual/sky_color", "#000000"),
        ],
    );
    nbt.push(0);
    nbt.push(0);
    nbt.push(0);
    nbt.push(0);
    gzip(&nbt)
}

#[test]
fn the_network_codec_yields_biome_climate_in_registry_order() {
    let biomes = nbt::biomes(&codec(&[
        ("minecraft:plains", 0, (0.8, 0.4, 1), ""),
        ("minecraft:frozen_ocean", 1, (0.0, 0.5, 1), "frozen"),
        ("minecraft:desert", 2, (2.0, 0.0, 0), "none"),
    ]))
    .unwrap();
    let entry =
        |name: &str, id, temperature, downfall, has_precipitation, frozen| nbt::CodecBiome {
            name: name.to_owned(),
            id,
            temperature,
            downfall,
            has_precipitation,
            frozen,
            colours: JavaSkyColours {
                sky: Some(0x78_A7_FF),
                ..JavaSkyColours::default()
            },
        };
    assert_eq!(
        biomes,
        vec![
            entry("minecraft:plains", 0, 0.8, 0.4, true, false),
            entry("minecraft:frozen_ocean", 1, 0.0, 0.5, true, true),
            entry("minecraft:desert", 2, 2.0, 0.0, false, false),
        ]
    );
    assert!(nbt::biomes(&codec(&[("minecraft:plains", 0, (0.8, 0.4, 1), "warm")])).is_err());
    assert!(nbt::biomes(&codec(&[("minecraft:plains", 0, (0.8, 0.4, 2), "")])).is_err());
}

#[test]
fn the_network_codec_yields_dimension_type_colours() {
    assert_eq!(
        nbt::dimensions(&codec(&[])).unwrap(),
        vec![nbt::CodecDimension {
            name: "minecraft:the_end".into(),
            colours: JavaSkyColours {
                sky: Some(0),
                fog: Some(0x18_13_18),
                water_fog: None,
            },
        }]
    );
}

/// The pinned source files, or `None` (with the missing-fixture notice) without the archive.
fn pinned_files() -> Option<SourceFiles> {
    let archive = pinned_archive()?;
    let manifest = std::fs::read(root().join(java_mapping::SOURCE_MANIFEST)).unwrap();
    Some(source::load(&root(), &manifest, Some(&archive)).unwrap())
}

#[test]
fn the_java_biome_registry_is_the_geyser_biome_set_with_its_climate_and_tags() {
    let Some(files) = pinned_files() else {
        return;
    };
    let geyser: BTreeMap<String, GeyserBiome> = serde_json::from_slice(&files.biomes).unwrap();
    let registry = java_biomes(&files, &geyser).unwrap();
    assert_eq!(registry.len(), geyser.len());
    let index = |name: &str| {
        registry
            .iter()
            .position(|biome| biome.name.as_ref() == name)
            .unwrap_or_else(|| panic!("no Java biome {name}"))
    };
    let plains = &registry[index("minecraft:plains")];
    assert_eq!(
        (
            plains.temperature,
            plains.downfall,
            plains.has_precipitation,
            plains.frozen
        ),
        (0.8, 0.4, true, false)
    );
    assert!(registry[index("minecraft:frozen_ocean")].frozen);
    assert_eq!(plains.colours.sky, Some(0x78_A7_FF));
    let pale = &registry[index("minecraft:pale_garden")].colours;
    assert_eq!(
        (pale.sky, pale.fog, pale.water_fog),
        (Some(0xB9_B9_B9), Some(0x81_77_70), Some(0x55_69_80))
    );
    let dimensions = dimension_colours(&files).unwrap();
    let [overworld, _, end] = dimensions;
    assert_eq!(overworld.fog, Some(0xC0_D8_FF));
    assert_eq!((end.sky, end.fog), (Some(0), Some(0x18_13_18)));
    let tags = biome_tags(&files, registry.len()).unwrap();
    let members = |tag: &str| {
        &tags
            .iter()
            .find(|(name, _)| name.as_ref() == tag)
            .unwrap_or_else(|| panic!("no biome tag {tag}"))
            .1
    };
    assert!(members("minecraft:is_ocean").contains(&(index("minecraft:ocean") as u16)));
    assert!(!members("minecraft:is_ocean").contains(&(index("minecraft:plains") as u16)));
}

#[test]
fn the_shipped_licenses_are_the_pinned_upstream_licenses() {
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root().join(java_mapping::SOURCE_MANIFEST)).unwrap())
            .unwrap();
    for (shipped, pinned) in [
        ("GeyserMC-mappings-MIT.txt", &manifest["license"]["sha256"]),
        (
            "GeyserMC-Geyser-MIT.txt",
            &manifest["geyser"]["files"]["license"]["sha256"],
        ),
    ] {
        let bytes = std::fs::read(root().join("assets/licenses").join(shipped)).unwrap();
        assert_eq!(
            format!("{:x}", Sha256::digest(&bytes)),
            pinned.as_str().unwrap(),
            "{shipped}"
        );
    }
}
