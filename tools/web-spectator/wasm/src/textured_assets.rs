use std::sync::Arc;

use assets::{NetworkIdMode, RuntimeAssets};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::canonical;

const MAX_CARRIER_BYTES: usize = 64 * 1024 * 1024;
const MAX_REGISTRY_BYTES: usize = 32 * 1024 * 1024;

/// Validated terrain visuals and pixels compiled by Cinnabar from its pinned vanilla pack.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
#[derive(Clone)]
pub struct TerrainAssets {
    pub(super) runtime: Arc<RuntimeAssets>,
    pub(super) canonical: Arc<canonical::CanonicalIndex>,
    pub(super) air: u32,
    #[cfg(target_arch = "wasm32")]
    pub(super) collision_records: Arc<[assets::RegistryRecord]>,
    #[cfg(target_arch = "wasm32")]
    pub(super) collision_halo: [[i32; 2]; 3],
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
impl TerrainAssets {
    /// Decodes and verifies the carrier and the matching canonical block registry once.
    #[cfg_attr(
        target_arch = "wasm32",
        wasm_bindgen::prelude::wasm_bindgen(constructor)
    )]
    pub fn new(carrier: &[u8], registry: &[u8], protocol: u32) -> Result<Self, String> {
        if carrier.len() > MAX_CARRIER_BYTES || registry.len() > MAX_REGISTRY_BYTES {
            return Err("terrain asset input exceeds browser admission limits".into());
        }
        let runtime = RuntimeAssets::decode(carrier).map_err(|error| error.to_string())?;
        let registry_sha: [u8; 32] = Sha256::digest(registry).into();
        if runtime.provenance().block_registry_sha256 != registry_sha {
            return Err(
                "terrain carrier and block registry have different source identities".into(),
            );
        }
        if runtime.provenance().source_manifest_sha256 != assets::vanilla_source_manifest_sha256() {
            return Err("terrain carrier does not match Cinnabar's pinned vanilla pack".into());
        }
        let records = assets::read_registry_for_protocol(registry, protocol)
            .map_err(|error| error.to_string())?;
        if runtime.visual_count() != records.len() {
            return Err("terrain carrier and block registry have different visual counts".into());
        }
        let air = runtime
            .air_network_id(NetworkIdMode::Sequential)
            .ok_or("terrain carrier does not contain one unambiguous air identity")?;
        let canonical = canonical::registry_index(&records)?;
        #[cfg(target_arch = "wasm32")]
        let mut collision_halo = [[0, 0]; 3];
        #[cfg(target_arch = "wasm32")]
        for shape in records
            .iter()
            .flat_map(|record| record.collision_seed.boxes.iter())
        {
            let lower = [shape.min_x, shape.min_y, shape.min_z];
            let upper = [shape.max_x, shape.max_y, shape.max_z];
            for axis in 0..3 {
                collision_halo[axis][0] =
                    collision_halo[axis][0].min(lower[axis].div_euclid(100_000_000));
                collision_halo[axis][1] = collision_halo[axis][1]
                    .max((i64::from(upper[axis]) + 99_999_999).div_euclid(100_000_000) as i32);
            }
        }
        Ok(Self {
            runtime: Arc::new(runtime),
            canonical: Arc::new(canonical),
            air,
            #[cfg(target_arch = "wasm32")]
            collision_records: Arc::from(records),
            #[cfg(target_arch = "wasm32")]
            collision_halo,
        })
    }

    /// Validates all streamed states against the matching compiled registry.
    /// Unknown states are rejected rather than replaced with approximate blocks.
    pub fn validate_arena(&self, input: &str) -> Result<(), String> {
        let arena = crate::model::Arena::parse(input)?;
        let ids = canonical::palette_ids(&self.canonical, &arena.palette)?;
        if ids.first().copied() != Some(self.air) {
            return Err("arena palette zero does not resolve to the compiled air state".into());
        }
        Ok(())
    }

    /// Small JSON descriptors for the layer-major RGBA8 texture pages.
    pub fn texture_pages(&self) -> Result<String, String> {
        #[derive(Serialize)]
        struct Page {
            page: usize,
            size: u32,
            layers: u32,
        }
        let pages = self
            .runtime
            .texture_pages()
            .iter()
            .enumerate()
            .map(|(page, value)| Page {
                page,
                size: value.texture.mips[0].size,
                layers: value.texture.layers,
            })
            .collect::<Vec<_>>();
        serde_json::to_string(&pages).map_err(|error| error.to_string())
    }

    /// Exports only mip zero. Texture Y=0 is the first pixel row; browser textures use flipY=false.
    pub fn texture_page_rgba8(&self, page: u32) -> Result<Vec<u8>, String> {
        self.runtime
            .texture_pages()
            .get(page as usize)
            .map(|value| value.texture.mips[0].rgba8.to_vec())
            .ok_or_else(|| "terrain texture page is out of range".into())
    }
}
