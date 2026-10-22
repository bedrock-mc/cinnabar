//! Measures production job capture without scheduling or executing workers.

use super::*;

const VERTICAL_CAMERA: [f32; 3] = [8.0, 72.0, 8.0];

/// Resident packed sections and current light shared by overlapping job neighbourhoods.
pub struct DispatchFixture {
    stream: WorldStream,
    keys: Vec<SubChunkKey>,
}

impl DispatchFixture {
    /// Installs a bounded mixed-palette terrain cohort; setup is outside the timed region.
    pub fn new(sections: usize) -> Self {
        Self::stacked(sections, 4, 0, RuntimeAssets::diagnostic())
    }

    /// Installs full-height columns for vertical mesh-admission comparisons.
    pub fn vertical_grid(side: usize) -> Self {
        let range = vanilla_dimension_range(0).unwrap();
        Self::stacked(
            side * side * range.sub_chunk_count,
            range.sub_chunk_count,
            range.base_sub_chunk_y,
            vertical_assets(),
        )
    }

    /// Builds the same mixed-palette sections and light halo for either layout.
    fn stacked(sections: usize, height: usize, base_y: i32, assets: RuntimeAssets) -> Self {
        assert!(sections > 0 && height > 0);
        let mut stream = WorldStream::new_with_assets(
            client_world::ingestion::WorldBootstrap {
                local_player_unique_id: 1,
                dimension: 0,
                local_player_runtime_id: 1,
                player_position: [8.0; 3],
                world_spawn_position: [8; 3],
                air_network_id: 0,
                block_network_ids_are_hashes: false,
            },
            Arc::new(assets),
            [0.0, crate::server_position::SAFE_SERVER_HEIGHT, 0.0],
            None,
        );
        let side = (sections.div_ceil(height) as f64).sqrt().ceil() as i32;
        let mut bytes = vec![9, 1, 0, 3];
        for _ in 0..128 {
            bytes.extend_from_slice(&0xaaaa_aaaa_u32.to_le_bytes());
        }
        bytes.extend([4, 0, 2]);
        let section = SubChunk::decode(&bytes, &world::RawBlockIds { air: 0 });
        let keys: Vec<_> = (0..sections)
            .map(|index| {
                let column = (index / height) as i32;
                SubChunkKey::new(
                    0,
                    column % side,
                    base_y + (index % height) as i32,
                    column / side,
                )
            })
            .collect();
        let halo: BTreeSet<_> = keys
            .iter()
            .flat_map(|key| key.mesh_neighbourhood_dependents())
            .collect();
        for key in halo {
            stream
                .authority
                .commit_sub_chunk(key, section.clone())
                .unwrap();
            stream.resident.insert(key);
            stream.lighting.block_generations.insert(key, 1);
            let mut light = SubChunkLight::uniform(0, 15, 1).unwrap();
            light.set(LightChannel::Block, 8, 8, 8, 7).unwrap();
            stream.lighting.store.insert_resident(key, light);
            stream.lighting.ownership.insert(
                key,
                LightOwnership {
                    block_generation: 1,
                    light_revision: 1,
                },
            );
            stream.lighting.direct_sky.insert(
                key,
                StoredDirectSky {
                    light_revision: 1,
                    mask: Arc::new(DirectSkyMask::Uniform(true)),
                },
            );
        }
        Self { stream, keys }
    }

    /// Applies a vertical radius around section four, keeping fixture setup untimed.
    pub fn set_vertical_distance(&mut self, chunks: u32) {
        self.stream.set_vertical_render_distance(chunks);
        self.stream.reevaluate_vertical_detail(VERTICAL_CAMERA);
    }

    /// Builds real meshes with production admission, snapshot capture and biome records.
    /// Returns meshed sections and bytes that their publication would upload.
    pub fn mesh_admitted(&mut self) -> (usize, u64) {
        let records = self.stream.biome_record_cache(0);
        let mut sections = 0;
        let mut bytes = 0;
        for &key in &self.keys {
            if !self.stream.mesh_requires_geometry(key) {
                continue;
            }
            let center = self.stream.authority.terrain().sub_chunk(key).unwrap();
            let halo = self.stream.mesh_light_halo(key).unwrap();
            let snapshot = self.stream.mesh_snapshot(key, center, halo);
            let biome = records.record(&snapshot.biomes);
            let mesh = snapshot.mesh(
                self.stream.classifier,
                self.stream.authority.runtime_assets(),
                self.stream.authority.network_id_mode(),
                &biome,
            );
            bytes += chunk_publication_byte_len(&mesh, &biome);
            sections += 1;
            std::hint::black_box(mesh);
        }
        (sections, bytes)
    }

    /// Measures a one-section camera crossing, including resumable mesh reconciliation.
    pub fn cross_vertical_band(&mut self) {
        let mut camera = VERTICAL_CAMERA;
        camera[1] += world::SUB_CHUNK_SIDE as f32;
        self.stream.reevaluate_vertical_detail(camera);
    }

    /// Captures and releases the same inputs prepared for each dispatched light job.
    pub fn capture_light(&self) {
        for &key in &self.keys {
            std::hint::black_box((
                self.stream.light_block_snapshot(key),
                self.stream.light_prior_snapshot(key),
            ));
        }
    }

    /// Captures and releases the same inputs prepared for each dispatched mesh job.
    pub fn capture_mesh(&self) {
        for &key in &self.keys {
            let center = self.stream.authority.terrain().sub_chunk(key).unwrap();
            let halo = self.stream.mesh_light_halo(key).unwrap();
            std::hint::black_box(self.stream.mesh_snapshot(key, center, halo));
        }
    }
}

/// Original opaque-cube visuals keep the vertical benchmark on the ordinary terrain path.
fn vertical_assets() -> RuntimeAssets {
    use assets::{
        BlockFlags, BlockOverlay, BlockVisual, ContributorRole, LightProperties, Material,
        NO_ANIMATION, NO_MODEL_TEMPLATE, TextureRef, VisualKind, VisualSupport,
    };
    let base = RuntimeAssets::diagnostic();
    let texture = base.texture_array().clone();
    base.with_block_overlay(
        1,
        &BlockOverlay {
            visuals: vec![BlockVisual {
                native_material: assets::native_material::NativeMaterialFacts::Unresolved,
                faces: [0; 6],
                flags: BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
                kind: VisualKind::Cube,
                support: VisualSupport::Exact,
                contributor_role: ContributorRole::Primary,
                model_template: NO_MODEL_TEMPLATE,
                animation: NO_ANIMATION,
                variant: 0,
                destroy: assets::compiled::block_destroy::BlockDestroyFacts::Unresolved,
            }],
            light_properties: vec![LightProperties::OPAQUE_DARK],
            materials: vec![Material {
                texture: TextureRef::new(1, 0).unwrap(),
                flags: 0,
                ..Material::unvaried()
            }],
            textures: vec![texture],
            ..Default::default()
        },
    )
    .unwrap()
}
