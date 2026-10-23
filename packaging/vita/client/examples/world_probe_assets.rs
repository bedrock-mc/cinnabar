//! Developer carrier integration. The full startup carrier set and streamed asset
//! ownership are not implemented here; this example loads one world carrier once.

use super::{Fixture, log};
use assets::{NetworkIdMode, RuntimeAssets, TextureRef};
use cinnabar_vita::{
    memory::{Budget, Pool},
    native::{GpuMesh, GpuTexture, Renderer},
    terrain::{CubeUpload, material_runs},
    texture::TextureLayer,
};
use meshing::{BlockClassifier, Neighbourhood, mesh_sub_chunk};
use std::{collections::BTreeMap, io::Read};

pub fn load(budget: &Budget) -> Result<(RuntimeAssets, Fixture), String> {
    let path =
        std::path::Path::new("ux0:data/cinnabar/assets").join(assets::carriers::WORLD.output);
    let mut file = match std::fs::File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            log(format_args!(
                "world carrier absent: using explicit diagnostic fixture"
            ));
            return Ok((RuntimeAssets::diagnostic(), Fixture::new()));
        }
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    let length = usize::try_from(file.metadata().map_err(|e| e.to_string())?.len())
        .map_err(|_| "world carrier exceeds address space")?;
    let _staging = budget
        .reserve(Pool::Main, length)
        .map_err(|e| format!("{e:?}"))?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(length).map_err(|e| e.to_string())?;
    let _padding = budget
        .reserve(Pool::Main, bytes.capacity() - length)
        .map_err(|e| format!("{e:?}"))?;
    bytes.resize(length, 0);
    file.read_exact(&mut bytes).map_err(|e| e.to_string())?;
    // The shared decoder verifies the seal, layout and references. Its internal
    // allocations are not yet admitted by the Vita ledger: no total-memory claim.
    let (assets, identity) = RuntimeAssets::decode_sealed(&bytes).map_err(|e| e.to_string())?;
    if assets.provenance() != assets::pinned_world_provenance() {
        return Err(
            "world carrier does not match the pinned content; run make world-assets".into(),
        );
    }
    let air = assets
        .air_network_id(NetworkIdMode::Sequential)
        .ok_or("missing unique air identity")?;
    let solid = (0..assets.visual_count() as u32)
        .find(|&id| {
            assets::pinned_block_identity(id).is_some_and(|(name, _)| name == "minecraft:bookshelf")
        })
        .ok_or("missing fixture bookshelf identity")?;
    let block = assets.resolve(NetworkIdMode::Sequential, solid);
    if block.kind() != assets::VisualKind::Cube {
        return Err("fixture block no longer has cube geometry".into());
    }
    log(format_args!(
        "world carrier bytes={length} seal={identity:02x?} air={air} solid={solid}; decoded asset allocations not in payload ledger"
    ));
    Ok((assets, Fixture::with_ids(air, solid)))
}

/// Uploaded textures are reused across this fixture's mesh replacements.
pub struct Materials<'a> {
    textures: BTreeMap<u32, GpuTexture<'a>>,
}

pub struct Draw<'a> {
    mesh: GpuMesh<'a>,
    texture: u32,
}

impl<'a> Materials<'a> {
    pub fn new() -> Self {
        Self {
            textures: BTreeMap::new(),
        }
    }

    pub fn upload(
        &mut self,
        renderer: &'a Renderer,
        budget: &Budget,
        fixture: &Fixture,
        assets: &RuntimeAssets,
    ) -> Result<Vec<Draw<'a>>, String> {
        let mesh = mesh_sub_chunk(
            &BlockClassifier::new(fixture.air),
            assets,
            NetworkIdMode::Sequential,
            &Neighbourhood::empty(),
            fixture.store.sub_chunk_ref(super::KEY).unwrap(),
        );
        let mut draws = Vec::new();
        for quads in material_runs(mesh.cube_quads()) {
            let material = assets.material(quads[0].material_id());
            // Fail this developer scenario instead of silently pretending to support
            // cutout, tint, rotation or animated materials in the native path.
            if material.flags != 0 || material.animation != assets::NO_ANIMATION {
                return Err(format!("unsupported fixture material: {material:?}"));
            }
            let key = material.texture.raw();
            if let std::collections::btree_map::Entry::Vacant(slot) = self.textures.entry(key) {
                let layer = TextureLayer::from_assets(
                    assets,
                    TextureRef::from_raw(key).map_err(|e| e.to_string())?,
                )
                .map_err(|e| format!("{e:?}"))?;
                slot.insert(
                    renderer
                        .upload_texture(&layer)
                        .map_err(|e| format!("{e:?}"))?,
                );
            }
            let upload = CubeUpload::new(quads, budget).map_err(|e| format!("{e:?}"))?;
            draws.push(Draw {
                mesh: renderer.upload(&upload).map_err(|e| format!("{e:?}"))?,
                texture: key,
            });
        }
        Ok(draws)
    }

    pub fn draw(&self, draws: &[Draw<'a>]) {
        for draw in draws {
            draw.mesh
                .draw_textured([0.; 3], [1.; 4], &self.textures[&draw.texture]);
        }
    }
}
