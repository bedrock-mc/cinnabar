use std::{fs, path::Path};

use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};
use serde_json::{Value, json};

use super::*;

struct Fixture {
    directory: tempfile::TempDir,
    records: Vec<RegistryRecord>,
    protocol: u32,
}

impl Fixture {
    fn new(tint: Value) -> Self {
        let target: Value =
            serde_json::from_slice(include_bytes!("../../../../../assets/bedrock-target.json"))
                .unwrap();
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let protocol = u32::try_from(target["wire_protocol"].as_u64().unwrap()).unwrap();
        let registry = assets::read_registry_for_protocol(
            &fs::read(root.join(target["artifacts"]["block_registry"].as_str().unwrap())).unwrap(),
            protocol,
        )
        .unwrap();
        let records = registry
            .into_iter()
            .filter(|record| {
                matches!(
                    record.name.as_ref(),
                    "minecraft:waterlily"
                        | "minecraft:ladder"
                        | "minecraft:stone"
                        | "minecraft:air"
                )
            })
            .collect();
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir_all(directory.path().join("textures/blocks")).unwrap();
        fs::write(
            directory.path().join("blocks.json"),
            serde_json::to_vec(&json!({
                "waterlily":{"textures":"pad", "carried_textures":"carried_pad"},
                "ladder":{"textures":"pad_alias"},
                "stone":{"textures":"stone"}
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            directory.path().join("textures/terrain_texture.json"),
            serde_json::to_vec(&json!({
                "texture_data": {
                    "pad":{"textures":[{"path":"textures/blocks/shared", "tint_color":tint}]},
                    "pad_alias":{"textures":"textures/blocks/shared"},
                    "carried_pad":{"textures":"textures/blocks/carried"},
                    "stone":{"textures":"textures/blocks/stone"}
                }
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            directory.path().join("textures/flipbook_textures.json"),
            "[]",
        )
        .unwrap();
        let mut shared = [255; 16 * 16 * 4];
        shared[3] = 0;
        write_png(directory.path(), "shared", &shared);
        write_png(directory.path(), "stone", &[255; 16 * 16 * 4]);
        write_png(directory.path(), "carried", &[127; 16 * 16 * 4]);
        Self {
            directory,
            records,
            protocol,
        }
    }

    fn compile(&self) -> (CompiledAssets, MaterialKeys) {
        let lights = vec![
            LightProperties::default();
            self.records
                .iter()
                .map(|record| record.sequential_id as usize + 1)
                .max()
                .unwrap()
        ];
        compile_pack_inner(
            self.directory.path(),
            &self.records,
            &lights,
            CompiledBiomeAssets::diagnostic(),
            self.protocol,
        )
        .unwrap()
    }

    fn lily(&self) -> &RegistryRecord {
        self.records
            .iter()
            .find(|record| is_record(record))
            .expect("active registry lily pad")
    }
}

fn write_png(root: &Path, name: &str, pixels: &[u8]) {
    let mut png = Vec::new();
    PngEncoder::new(&mut png)
        .write_image(pixels, 16, 16, ExtendedColorType::Rgba8)
        .unwrap();
    fs::write(root.join(format!("textures/blocks/{name}.png")), png).unwrap();
}

fn pixel(assets: &CompiledAssets, material: u32, offset: usize) -> [u8; 4] {
    let texture = assets.materials[material as usize].texture;
    let mip = &assets.texture_pages[texture.page() as usize].texture.mips[0];
    let start = texture.layer() as usize * (mip.size * mip.size * 4) as usize + offset * 4;
    mip.rgba8[start..start + 4].try_into().unwrap()
}

#[test]
fn current_registry_lily_pad_uses_native_planes_tint_and_isolated_materials() {
    let fixture = Fixture::new(json!("#208030"));
    let lily = fixture.lily();
    let fallback = visuals::fallback::inventory(fixture.protocol).unwrap();
    assert!(
        !fallback.contains(lily),
        "exact lily route must supersede provisional cuboid"
    );
    assert_eq!(fallback.material_flags(lily), None);
    let (compiled, keys) = fixture.compile();
    let visual = compiled.visuals[lily.sequential_id as usize];
    assert_eq!(visual.kind, VisualKind::Model);
    assert_eq!(visual.support, VisualSupport::Exact);
    assert!(
        !visual
            .flags
            .intersects(BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE)
    );
    let template = compiled.model_templates[visual.model_template as usize];
    assert_eq!(template.flags, assets::MODEL_TEMPLATE_FLAG_LILY_PAD);
    assert_eq!(template.quad_count, 2);
    let quads =
        &compiled.model_quads[template.quad_start as usize..template.quad_start as usize + 2];
    let expected = planes(quads[0].material);
    assert_eq!(quads[0], expected[0]);
    assert_eq!(quads[1].positions, expected[1].positions);
    assert_eq!(quads[1].uvs, expected[1].uvs);
    assert_eq!(quads[1].flags, expected[1].flags);
    assert_eq!(quads[0].material, quads[1].material);
    for quad in quads {
        assert_eq!(
            compiled.materials[quad.material as usize].flags,
            MATERIAL_FLAG_ALPHA_CUTOUT
        );
        assert!(
            quad.positions
                .iter()
                .all(|position| position[1] == PLANE_HEIGHT)
        );
        assert!(keys.materials("pad").contains(&quad.material));
        assert!(!keys.materials("pad_alias").contains(&quad.material));
    }
    assert_eq!(visual.faces, [quads[0].material; 6]);
    let decoded_keys = MaterialKeys::from_json(
        &keys.to_json(compiled.materials.len() as u32),
        compiled.materials.len(),
    )
    .unwrap();
    assert_eq!(
        decoded_keys.fixed_tints().collect::<Vec<_>>(),
        [("pad", [32, 128, 48])]
    );
    assert_eq!(pixel(&compiled, quads[0].material, 1), [32, 128, 48, 255]);
    assert_eq!(pixel(&compiled, quads[1].material, 1), [32, 128, 48, 255]);
    assert_eq!(pixel(&compiled, quads[0].material, 0)[3], 0);
    assert_eq!(pixel(&compiled, quads[1].material, 0)[3], 0);
    for &alias in keys.materials("pad_alias") {
        assert_eq!(
            pixel(&compiled, alias, 1),
            [255; 4],
            "untinted shared source must remain unchanged"
        );
    }
    assert!(
        !keys.materials("pad_alias").is_empty(),
        "exercise material dedup alias"
    );
    let pack = read_pack(fixture.directory.path()).unwrap();
    assert!(pack.terrain.get_clamped_carried("pad", 0).is_none());
    assert_eq!(
        pack.terrain.get_clamped_carried("carried_pad", 0),
        Some(("textures/blocks/carried", None))
    );
    let decoded = assets::RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    assert_eq!(
        decoded.model_templates()[visual.model_template as usize],
        template
    );
}

#[test]
fn lily_pad_plane_windings_and_uv_associations_are_native() {
    let [top, bottom] = planes(17);
    assert_eq!(
        top.positions,
        [
            [0, PLANE_HEIGHT, 256],
            [256, PLANE_HEIGHT, 256],
            [256, PLANE_HEIGHT, 0],
            [0, PLANE_HEIGHT, 0]
        ]
    );
    assert_eq!(top.uvs, [[0, 0], [4096, 0], [4096, 4096], [0, 4096]]);
    let normal_y = |quad: &ModelQuad| {
        let a = quad.positions[0];
        let b = quad.positions[1];
        let c = quad.positions[2];
        i32::from(b[2] - a[2]) * i32::from(c[0] - a[0])
            - i32::from(b[0] - a[0]) * i32::from(c[2] - a[2])
    };
    assert!(normal_y(&top) > 0);
    assert!(normal_y(&bottom) < 0);
    for (position, uv) in top.positions.into_iter().zip(top.uvs) {
        assert!(
            bottom
                .positions
                .into_iter()
                .zip(bottom.uvs)
                .any(|pair| pair == (position, uv))
        );
    }
}

#[test]
fn malformed_lily_pad_fixed_tint_remains_diagnostic_not_fallback() {
    for tint in [
        json!("#not-a-colour"),
        json!({"unsupported":"#208030"}),
        json!([0.125, 0.5, 0.1875]),
    ] {
        let fixture = Fixture::new(tint);
        let (compiled, _) = fixture.compile();
        let visual = compiled.visuals[fixture.lily().sequential_id as usize];
        assert_eq!(visual.kind, VisualKind::Diagnostic);
        assert_ne!(visual.support, VisualSupport::VanillaFallback);
        assert!(
            compiled
                .model_templates
                .iter()
                .all(|template| template.flags != assets::MODEL_TEMPLATE_FLAG_LILY_PAD)
        );
    }
}

#[test]
fn lily_pad_unreviewed_positional_textures_stay_diagnostic() {
    let fixture = Fixture::new(json!("#208030"));
    let path = fixture
        .directory
        .path()
        .join("textures/terrain_texture.json");
    let mut terrain: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    terrain["texture_data"]["pad"]["textures"] =
        json!({"variations":["textures/blocks/shared", "textures/blocks/stone"]});
    fs::write(path, serde_json::to_vec(&terrain).unwrap()).unwrap();
    let (compiled, _) = fixture.compile();
    assert_eq!(
        compiled.visuals[fixture.lily().sequential_id as usize].kind,
        VisualKind::Diagnostic
    );
}
