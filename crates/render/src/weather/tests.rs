use super::*;

struct Flat {
    surface_y: i32,
    temperature: f32,
    downfall: f32,
}

impl ColumnSampler for Flat {
    fn sample(&mut self, _x: i32, _z: i32) -> Option<ColumnSample> {
        Some(ColumnSample {
            surface_y: self.surface_y,
            temperature: self.temperature,
            downfall: self.downfall,
        })
    }
}

fn rainy() -> Flat {
    Flat {
        surface_y: 64,
        temperature: 0.8,
        downfall: 0.4,
    }
}

#[test]
fn level_approaches_the_target_without_overshoot() {
    assert_eq!(approach_level(0.0, 1.0, 0.25), 0.25);
    assert_eq!(approach_level(0.9, 1.0, 0.25), 1.0);
    assert!((approach_level(1.0, 0.0, 0.4) - 0.6).abs() < 1.0e-6);
    assert_eq!(approach_level(f32::NAN, 0.5, 1.0), 0.5);
    assert_eq!(approach_level(0.3, 0.8, f32::NAN), 0.3);
}

#[test]
fn kind_constants_match_the_vanilla_tables() {
    assert_eq!(
        (
            RAIN_PARAMS.fall_speed,
            RAIN_PARAMS.length,
            RAIN_PARAMS.width,
            RAIN_PARAMS.wind
        ),
        (0.6, 0.5, 0.1, 0.1)
    );
    assert_eq!(
        (
            SNOW_PARAMS.fall_speed,
            SNOW_PARAMS.length,
            SNOW_PARAMS.width,
            SNOW_PARAMS.wind
        ),
        (0.05, 0.2, 0.2, 0.05)
    );
    assert_eq!(RAIN_PARAMS.uv_rect, [0.0, 0.125, 0.125, 0.5]);
    assert_eq!(SNOW_PARAMS.uv_rect, [0.0, 0.0, 0.125, 0.125]);
    let [width, length] = RAIN_PARAMS.dimensions();
    assert_eq!(width, 0.1);
    assert!((length - 0.5 / 0.6).abs() < 1.0e-6);
    assert_eq!(SNOW_PARAMS.dimensions(), [0.2, 4.0]);
    assert_eq!(
        (PARTICLE_BOX, PARTICLE_MESH_QUADS, PARTICLE_POOL),
        (30.0, 2500, 925)
    );
}

#[test]
fn layer_density_follows_the_smoothed_intensity() {
    assert_eq!(particles_per_layer(0.0), 0);
    assert_eq!(particles_per_layer(f32::NAN), 0);
    // Full rain over every lattice sample: 27 * 0.5 = 13.5, halved, / 10, * 925.
    assert_eq!(particles_per_layer(13.5), 624);
    assert_eq!(particles_per_layer(1.0e9), PARTICLE_MESH_QUADS as u32);
}

#[test]
fn intensity_eases_halfway_each_tick() {
    let mut sim = PrecipitationSim::new(1);
    sim.tick([13.5, 0.0], 0.0);
    assert_eq!(sim.intensity(), [6.75, 0.0]);
    sim.tick([13.5, f32::NAN], 0.05);
    assert_eq!(sim.intensity(), [10.125, 0.0]);
}

#[test]
fn mesh_fills_the_offset_box_with_eight_sprites() {
    let mesh = particle_mesh(7);
    assert_eq!(mesh.len(), PARTICLE_MESH_QUADS);
    for [x, y, z, sprite] in &mesh {
        for axis in [x, y, z] {
            assert!((PARTICLE_BOX..2.0 * PARTICLE_BOX).contains(axis));
        }
        assert!((0.0..8.0).contains(sprite) && sprite.fract() == 0.0);
    }
    assert!(mesh.iter().any(|quad| quad[3] == 7.0));
}

#[test]
fn rain_falls_about_a_fall_speed_per_tick() {
    let mut sim = PrecipitationSim::new(3);
    for tick in 0..20 {
        sim.tick([13.5, 0.0], tick as f32 / 20.0);
    }
    let mut records = Vec::new();
    sim.frame([0.0; 3], [0.0; 3], 1.0, &mut records);
    assert_eq!(records.len(), LAYERS_PER_KIND);
    for record in &records {
        assert_eq!(record.kind, 0);
        assert_eq!(record.uv_rect, RAIN_PARAMS.uv_rect);
        let fall = -record.velocity[1];
        assert!((0.6 * 0.75 - 0.02..=0.6 * 1.25 + 0.02).contains(&fall));
        assert!(record.velocity[0].abs() <= 0.1 * 1.25 && record.velocity[2].abs() <= 0.1 * 1.25);
        assert!(
            record.base_offset[..3]
                .iter()
                .all(|v| (0.0..=30.0).contains(v))
        );
    }
}

#[test]
fn base_offsets_anchor_particles_to_the_world() {
    let sim = PrecipitationSim::new(5);
    let (mut here, mut moved) = (Vec::new(), Vec::new());
    let mut stepped = sim.clone();
    stepped.intensity = [5.0, 0.0];
    stepped.frame([100.0, 70.0, -40.0], [0.0; 3], 0.0, &mut here);
    stepped.frame([101.0, 70.0, -40.0], [0.0; 3], 0.0, &mut moved);
    for (a, b) in here.iter().zip(&moved) {
        let shift = wrap(a.base_offset[0] - b.base_offset[0], PARTICLE_BOX);
        assert!(
            (shift - 1.0).abs() < 1.0e-3,
            "camera step shifts offset by one block"
        );
        assert_eq!(a.base_offset[1], b.base_offset[1]);
    }
}

#[test]
fn forward_offset_centres_the_box_half_a_box_ahead() {
    assert_eq!(
        precipitation_forward_offset([0.0, 0.0, -1.0]),
        [0.0, 0.0, -15.0]
    );
    assert_eq!(
        precipitation_forward_offset([f32::NAN, 1.0, 0.0]),
        [0.0, 15.0, 0.0]
    );
}

#[test]
fn lattice_matches_the_vanilla_offsets_and_weights() {
    assert_eq!(PRECIPITATION_SAMPLE_OFFSETS[0], [0, 0, 0]);
    assert_eq!(PRECIPITATION_SAMPLE_OFFSETS[1], [-12, 0, 0]);
    assert_eq!(PRECIPITATION_SAMPLE_OFFSETS[2], [-8, 0, -8]);
    assert_eq!(PRECIPITATION_SAMPLE_OFFSETS[9], [0, -3, 0]);
    assert_eq!(PRECIPITATION_SAMPLE_OFFSETS[26], [-8, 3, 8]);
    let mut samples = vec![Some((0.8, 0.4, 64)); 27];
    let full = average_precipitation(&samples);
    assert_eq!(
        full,
        PrecipitationMix {
            rain: 1.0,
            snow: 0.0
        }
    );
    assert_eq!(full.lattice_weights(1.0), [13.5, 0.0]);
    for sample in samples.iter_mut().take(9) {
        *sample = Some((0.0, 0.5, 64));
    }
    for sample in samples.iter_mut().skip(9).take(9) {
        *sample = None;
    }
    let mix = average_precipitation(&samples);
    assert!((mix.rain - 9.0 / 27.0).abs() < 1.0e-6 && (mix.snow - 9.0 / 27.0).abs() < 1.0e-6);
}

#[test]
fn occlusion_hides_the_other_kind_and_dry_columns() {
    let rain = column_heights(Some(ColumnSample {
        surface_y: 70,
        temperature: 0.8,
        downfall: 0.4,
    }));
    assert_eq!(rain, [70, OCCLUSION_BLOCKED]);
    let snow = column_heights(Some(ColumnSample {
        surface_y: 70,
        temperature: 0.0,
        downfall: 0.4,
    }));
    assert_eq!(snow, [OCCLUSION_BLOCKED, 70]);
    let desert = column_heights(Some(ColumnSample {
        surface_y: 70,
        temperature: 2.0,
        downfall: 0.0,
    }));
    assert_eq!(desert, [OCCLUSION_BLOCKED; 2]);
    assert_eq!(column_heights(None), [OCCLUSION_OPEN; 2]);
}

#[test]
fn occlusion_grid_recentres_and_keeps_overlap() {
    let mut grid = OcclusionGrid::default();
    let mut cursor = 0;
    let origin = OcclusionGrid::origin_for([0.5, 70.0, 0.5]);
    assert_eq!(origin, [-32, -32]);
    grid.update(origin, &mut rainy(), &mut cursor, 0);
    assert_eq!(grid.column(0, 0), Some([64, OCCLUSION_BLOCKED]));
    assert_eq!(grid.column(31, 31), Some([64, OCCLUSION_BLOCKED]));
    assert_eq!(grid.column(32, 0), None);
    let mut higher = Flat {
        surface_y: 90,
        ..rainy()
    };
    grid.update([-31, -32], &mut higher, &mut cursor, 0);
    assert_eq!(grid.column(0, 0), Some([64, OCCLUSION_BLOCKED]), "kept");
    assert_eq!(grid.column(32, 0), Some([90, OCCLUSION_BLOCKED]), "exposed");
    grid.update([-31, -32], &mut higher, &mut cursor, 4096);
    assert_eq!(
        grid.column(0, 0),
        Some([90, OCCLUSION_BLOCKED]),
        "refreshed"
    );
}

#[test]
fn splashes_land_on_rain_columns_and_scale_with_level() {
    let mut grid = OcclusionGrid::default();
    grid.update([-32, -32], &mut rainy(), &mut 0, 0);
    let mut out = Vec::new();
    pick_rain_splashes(&grid, 1.0, 7, &mut out);
    assert_eq!(out.len(), 4);
    for position in &out {
        assert!(position[0].abs() <= 11.0 && position[2].abs() <= 11.0);
        assert_eq!(position[1], 64.0);
    }
    pick_rain_splashes(&grid, 0.0, 7, &mut out);
    assert!(out.is_empty());
    let mut cold = OcclusionGrid::default();
    cold.update(
        [-32, -32],
        &mut Flat {
            temperature: 0.0,
            ..rainy()
        },
        &mut 0,
        0,
    );
    pick_rain_splashes(&cold, 1.0, 7, &mut out);
    assert!(out.is_empty(), "snow does not splash");
}

#[test]
fn simplex_noise_stays_in_range() {
    let noise = Simplex::new(9);
    let mut peak = 0.0_f32;
    for step in 0..2000 {
        let value = noise.sample(step as f32 * 0.037, 0.5);
        assert!(value.abs() <= 1.0);
        peak = peak.max(value.abs());
    }
    assert!(peak > 0.3);
}
