use super::super::resources::vanilla::{VanillaImage, VanillaTextures, bedrock_path};
use super::celestial::Celestial;
use super::clouds::{CloudCache, CloudFrame, cloud_quads};
use super::frame::EffectCamera;
use super::lines::{outline, outline_width, widen};
use super::particles::{instance_light, particle_quads};
use super::settings::ParticleOrdering;
use super::stars::{JavaRandom, STAR_QUADS};
use super::vertices::{FULL_BRIGHT, Mesh};
use super::weather::{rain_columns, weather_quads};
use super::*;
use crate::shaderpack::draws::{DrawKind, resolve_program};
use crate::shaderpack::geometry::IRIS_VERTEX_STRIDE;
use ::particles::ParticleInstance;
use assets::{AtmosphereRole, AtmosphereTexture};
use bevy::math::{DVec3, Mat4, Vec3};
use render_model::{BlockEntityVertex, OcclusionGrid, PrecipitationLayerRecord};
use shaderpack::ShaderProperties;
use std::collections::BTreeMap;
use std::sync::Arc;

const WORDS: usize = (IRIS_VERTEX_STRIDE / 4) as usize;

fn resolve(effect: Effect, provided: &[&str]) -> Option<&'static str> {
    resolve_program(DrawKind::Effect(effect).program(), |n| {
        provided.contains(&n)
    })
}

fn positions(mesh: &Mesh) -> Vec<Vec3> {
    mesh.words
        .as_chunks::<WORDS>()
        .0
        .iter()
        .map(|v| {
            Vec3::new(
                f32::from_bits(v[0]),
                f32::from_bits(v[1]),
                f32::from_bits(v[2]),
            )
        })
        .collect()
}

fn uvs(mesh: &Mesh) -> Vec<[f32; 2]> {
    mesh.words
        .as_chunks::<WORDS>()
        .0
        .iter()
        .map(|v| {
            let unorm = |bits: u32| bits as f32 / 65535.0;
            [unorm(v[4] & 0xffff), unorm(v[4] >> 16)]
        })
        .collect()
}

fn properties(text: &str) -> ShaderProperties {
    ShaderProperties::parse(text, &BTreeMap::new())
}

#[test]
fn every_effect_falls_back_along_iris_chains() {
    let bsl = [
        "gbuffers_basic",
        "gbuffers_textured",
        "gbuffers_skybasic",
        "gbuffers_skytextured",
        "gbuffers_clouds",
        "gbuffers_damagedblock",
        "gbuffers_entities",
        "gbuffers_weather",
        "gbuffers_terrain",
    ];
    assert_eq!(resolve(Effect::Sun, &bsl), Some("gbuffers_skytextured"));
    assert_eq!(resolve(Effect::Moon, &bsl), Some("gbuffers_skytextured"));
    assert_eq!(resolve(Effect::Stars, &bsl), Some("gbuffers_skybasic"));
    assert_eq!(resolve(Effect::Clouds, &bsl), Some("gbuffers_clouds"));
    assert_eq!(resolve(Effect::Weather, &bsl), Some("gbuffers_weather"));
    assert_eq!(
        resolve(Effect::DamagedBlock, &bsl),
        Some("gbuffers_damagedblock")
    );
    assert_eq!(
        resolve(Effect::Lightning, &bsl),
        Some("gbuffers_entities"),
        "lightning → entities"
    );
    assert_eq!(
        resolve(Effect::Particles, &bsl),
        Some("gbuffers_textured"),
        "particles → textured_lit → textured"
    );
    assert_eq!(
        resolve(Effect::ParticlesTranslucent, &bsl),
        Some("gbuffers_textured"),
        "particles_translucent → particles → …"
    );
    assert_eq!(
        resolve(Effect::Outline, &bsl),
        Some("gbuffers_basic"),
        "line → basic"
    );

    let basic = ["gbuffers_basic"];
    for effect in Effect::ALL {
        assert_eq!(
            resolve(effect, &basic),
            Some("gbuffers_basic"),
            "{effect:?}"
        );
    }
    let terrain_only = ["gbuffers_terrain"];
    assert_eq!(
        resolve(Effect::DamagedBlock, &terrain_only),
        Some("gbuffers_terrain")
    );
    for effect in Effect::ALL
        .into_iter()
        .filter(|e| *e != Effect::DamagedBlock)
    {
        assert_eq!(resolve(effect, &terrain_only), None, "{effect:?}");
    }
    let own = [
        "gbuffers_particles",
        "gbuffers_particles_translucent",
        "gbuffers_lightning",
        "gbuffers_line",
    ];
    assert_eq!(resolve(Effect::Particles, &own), Some("gbuffers_particles"));
    assert_eq!(
        resolve(Effect::ParticlesAdditive, &own),
        Some("gbuffers_particles_translucent")
    );
    assert_eq!(resolve(Effect::Lightning, &own), Some("gbuffers_lightning"));
    assert_eq!(resolve(Effect::Outline, &own), Some("gbuffers_line"));
}

#[test]
fn effect_state_follows_iris_shader_keys() {
    assert_eq!(Effect::Sun.render_stage(), 4);
    assert_eq!(Effect::Moon.render_stage(), 5);
    assert_eq!(Effect::Stars.render_stage(), 6);
    assert_eq!(Effect::Particles.render_stage(), 19);
    assert_eq!(Effect::Clouds.render_stage(), 20);
    assert_eq!(Effect::Weather.render_stage(), 21);
    assert_eq!(Effect::Sun.default_alpha_test(), None);
    assert_eq!(Effect::Weather.default_alpha_test(), Some(ONE_TENTH_ALPHA));
    assert_eq!(Effect::Clouds.default_alpha_test(), Some(ONE_TENTH_ALPHA));
    assert_eq!(Effect::Outline.default_alpha_test(), None);
    assert_eq!(Effect::Particles.default_blend(), BlendMode::Off);
    assert_eq!(Effect::Weather.default_blend(), TRANSLUCENT_BLEND);
    assert_eq!(Effect::Sun.default_blend(), CELESTIAL_BLEND);
    assert!(
        !DrawKind::Effect(Effect::Moon).depth(),
        "sky effects skip depth"
    );
    assert!(DrawKind::Effect(Effect::Weather).depth());
    assert!(!Effect::Weather.depth_write(false));
    assert!(Effect::Weather.depth_write(true), "rain.depth writes depth");
    assert!(DrawKind::Effect(Effect::Clouds).culls());
    assert!(DrawKind::Effect(Effect::Particles).lightmap_matrix());
}

#[test]
fn directives_select_effects_and_particle_order() {
    let defaults = EffectSettings::read(&properties(""), true, None);
    assert_eq!(
        defaults.particles,
        ParticleOrdering::After,
        "deferred packs"
    );
    assert_eq!(defaults.clouds, CloudSetting::Default);
    assert_eq!(defaults.lightning_entity, -1);
    assert!(Effect::ALL.into_iter().all(|e| defaults.draws(e)));
    assert_eq!(
        EffectSettings::read(&properties(""), false, None).particles,
        ParticleOrdering::Mixed
    );
    let mixed = EffectSettings::read(&properties("particles.ordering=mixed\n"), true, Some(10101));
    assert_eq!(mixed.particles, ParticleOrdering::Mixed);
    assert!(!mixed.after_deferred(Effect::Particles));
    assert!(mixed.after_deferred(Effect::ParticlesTranslucent));
    assert!(mixed.after_deferred(Effect::Clouds));
    assert!(!mixed.after_deferred(Effect::Outline));
    assert_eq!(mixed.entity_id(Effect::Lightning), 10101);
    assert_eq!(mixed.entity_id(Effect::Particles), -1);
    let before = EffectSettings::read(&properties("particles.before.deferred=true\n"), true, None);
    assert_eq!(before.particles, ParticleOrdering::Before);
    assert!(!before.after_deferred(Effect::ParticlesAdditive));
    assert_eq!(
        EffectSettings::read(&properties("separateEntityDraws=true\n"), true, None).particles,
        ParticleOrdering::Mixed
    );
    let off = EffectSettings::read(
        &properties(
            "clouds=off\nsun=false\nmoon=false\nstars=false\nweather=false\nrain.depth=true\n",
        ),
        true,
        None,
    );
    for effect in [
        Effect::Clouds,
        Effect::Sun,
        Effect::Moon,
        Effect::Stars,
        Effect::Weather,
    ] {
        assert!(!off.draws(effect), "{effect:?}");
        assert!(!DrawKind::Effect(effect).drawn_by(&off));
    }
    assert!(off.rain_depth);
    assert!(DrawKind::TerrainSolid.drawn_by(&off));
    assert_eq!(
        EffectSettings::read(&properties("clouds=fast\n"), true, None).clouds,
        CloudSetting::Fast
    );
    assert_eq!(
        EffectSettings::read(&properties("clouds = fancy\n"), true, None).clouds,
        CloudSetting::Fancy
    );
}

#[test]
fn minecraft_textures_resolve_to_the_bedrock_paths_cinnabar_loaded() {
    assert_eq!(
        bedrock_path("minecraft:textures/environment/clouds.png"),
        Ok("textures/environment/clouds.png")
    );
    assert!(bedrock_path("minecraft:textures/atlas/blocks.png").is_err());
    assert!(bedrock_path("example:textures/sky/stars.png").is_err());
    assert!(bedrock_path("minecraft:shaders/core/foo.vsh").is_err());
    let clouds = VanillaImage {
        width: 2,
        height: 1,
        rgba8: Arc::from(&[1u8, 2, 3, 4, 5, 6, 7, 8][..]),
    };
    let vanilla =
        VanillaTextures::default().with("textures/environment/clouds.tga", clouds.clone());
    assert_eq!(
        vanilla.resolve("minecraft:textures/environment/clouds.png"),
        Ok(&clouds),
        "the extension is not compared"
    );
    assert!(
        vanilla
            .resolve("minecraft:textures/environment/sun.png")
            .is_err()
    );
}

#[test]
fn java_random_matches_java_util_random() {
    assert_eq!(JavaRandom::new(0).next_int(), -1_155_484_576);
    assert_eq!(JavaRandom::new(42).next_int(), -1_170_105_035);
    assert!((JavaRandom::new(0).next_float() - 0.730_967_76).abs() < 1e-7);
    assert!((JavaRandom::new(0).next_double() - 0.730_967_787_376_657).abs() < 1e-15);
}

#[test]
fn stars_lie_on_javas_hundred_block_shell() {
    assert!(STAR_QUADS.len() > 500 && STAR_QUADS.len() < 1500);
    for star in STAR_QUADS.iter() {
        let centre = star.iter().copied().sum::<Vec3>() / 4.0;
        assert!((centre.length() - 100.0).abs() < 0.01);
        let half = (star[0] - star[2]).length() / 2.0_f32.sqrt() / 2.0;
        assert!((0.149..0.251).contains(&half), "size {half}");
        let facing = (star[1] - star[0]).cross(star[2] - star[1]).normalize();
        assert!(
            facing.dot(centre.normalize()).abs() > 0.999,
            "faces the camera"
        );
    }
}

#[test]
fn sun_and_moon_follow_javas_celestial_pose_and_moon_phase() {
    let celestial = |angle, moon_phase| Celestial {
        angle,
        sun_path_rotation: 0.0,
        moon_phase,
        rain: 0.25,
    };
    let centre = |mesh: &Mesh| positions(mesh).into_iter().sum::<Vec3>() / 4.0;
    let mut noon = Mesh::default();
    celestial(0.0, 0).sun(&mut noon);
    assert!(centre(&noon).abs_diff_eq(Vec3::new(0.0, 100.0, 0.0), 1e-3));
    let mut sunset = Mesh::default();
    celestial(0.25, 0).sun(&mut sunset);
    assert!(
        centre(&sunset).abs_diff_eq(Vec3::new(-100.0, 0.0, 0.0), 1e-3),
        "Java's sun sets in the west"
    );
    let colour = noon.words[3].to_le_bytes();
    assert_eq!(colour, [255, 255, 255, 191], "alpha is 1 - rain");
    let mut moon = Mesh::default();
    celestial(0.0, 5).moon(&mut moon);
    assert!(centre(&moon).abs_diff_eq(Vec3::new(0.0, -100.0, 0.0), 1e-3));
    let uv = uvs(&moon);
    let (min_u, max_u) = (
        uv.iter().map(|t| t[0]).fold(1.0, f32::min),
        uv.iter().map(|t| t[0]).fold(0.0, f32::max),
    );
    let (min_v, max_v) = (
        uv.iter().map(|t| t[1]).fold(1.0, f32::min),
        uv.iter().map(|t| t[1]).fold(0.0, f32::max),
    );
    assert!(
        (min_u - 0.25).abs() < 1e-4 && (max_u - 0.5).abs() < 1e-4,
        "column 1"
    );
    assert!(
        (min_v - 0.5).abs() < 1e-4 && (max_v - 1.0).abs() < 1e-4,
        "row 1"
    );
    assert_eq!(celestial(0.0, 0).star_brightness(), 0.0, "no stars at noon");
    assert!(celestial(0.5, 0).star_brightness() > 0.3);
    let mut stars = Mesh::default();
    celestial(0.0, 0).stars(&mut stars);
    assert!(stars.indices.is_empty());
}

fn instance(lit: bool) -> ParticleInstance {
    ParticleInstance {
        center_light: [10.5, 70.0, -3.0, (7 | (12 << 4)) as f32],
        axis_x: [0.25, 0.0, 0.0, 0.0],
        axis_y: [0.0, 0.5, 0.0, if lit { 1.0 } else { 0.0 }],
        uv: [0.5, 0.25, 0.125, 0.0625],
        texture_bounds: [0.0; 4],
        color: [1.0, 0.5, 0.25, 0.75],
    }
}

#[test]
fn particles_reuse_mesh_storage_without_per_instance_allocations() {
    for texture_bounds in [
        [0.0; 4],
        [0.4, 0.3, 0.2, 0.4],
        [0.5, 0.5, 1.0 / 256.0, 1.0 / 256.0],
    ] {
        for uv in [
            [0.2, 0.1, 0.6, 0.8],
            [0.8, 0.9, -0.6, -0.8],
            [0.5, 0.5, 0.0, 0.0],
            [0.5, 0.5, 0.01, 0.01],
        ] {
            let instances = [ParticleInstance {
                uv,
                texture_bounds,
                ..instance(true)
            }; 64];
            let mut mesh = Mesh::default();
            particle_quads(&mut mesh, &instances, Vec3::ZERO, 256);
            let expected_words = mesh.words.clone();
            let expected_indices = mesh.indices.clone();
            mesh.clear();
            let (_, allocations) = test_alloc::count(|| {
                particle_quads(&mut mesh, &instances, Vec3::ZERO, 256);
            });
            assert_eq!(allocations, 0, "UV {uv:?}, bounds {texture_bounds:?}");
            assert_eq!(mesh.words, expected_words);
            assert_eq!(mesh.indices, expected_indices);
        }
    }
}

#[test]
fn particles_skip_non_finite_uvs_without_losing_valid_instances() {
    let valid = instance(false);
    let mut expected = Mesh::default();
    particle_quads(&mut expected, &[valid], Vec3::ZERO, 256);
    for bounds in [[0.0; 4], [0.25, 0.25, 0.5, 0.5]] {
        for uv in [
            [f32::INFINITY, 0.0, f32::NEG_INFINITY, 1.0],
            [0.0, f32::NAN, 1.0, 1.0],
            [0.0, 0.0, f32::INFINITY, 1.0],
            [f32::MAX, 0.0, f32::MAX, 1.0],
        ] {
            let invalid = ParticleInstance {
                uv,
                texture_bounds: bounds,
                ..valid
            };
            let mut mesh = Mesh::default();
            particle_quads(&mut mesh, &[invalid, valid], Vec3::ZERO, 256);
            assert_eq!(mesh.words, expected.words);
            assert_eq!(mesh.indices, expected.indices);
        }
    }
}

#[test]
fn particles_expand_like_the_vanilla_shader() {
    assert_eq!(instance_light(&instance(true)), [7.0 * 16.0, 12.0 * 16.0]);
    assert_eq!(instance_light(&instance(false)), FULL_BRIGHT);
    let mut mesh = Mesh::default();
    particle_quads(
        &mut mesh,
        &[instance(true)],
        Vec3::new(10.0, 70.0, -3.0),
        256,
    );
    assert_eq!(mesh.indices, [0, 1, 2, 0, 2, 3]);
    let corners = positions(&mesh);
    assert!(corners[0].abs_diff_eq(Vec3::new(0.25, -0.5, 0.0), 1e-6));
    assert!(corners[2].abs_diff_eq(Vec3::new(0.75, 0.5, 0.0), 1e-6));
    let uv = uvs(&mesh);
    assert!((uv[0][0] - 0.5).abs() < 1e-4 && (uv[0][1] - 0.3125).abs() < 1e-4);
    assert!((uv[2][0] - 0.625).abs() < 1e-4 && (uv[2][1] - 0.25).abs() < 1e-4);
}

#[test]
fn snow_streaks_stop_at_the_column_surface() {
    let layer = PrecipitationLayerRecord {
        base_offset: [0.0; 4],
        velocity: [0.0, -1.0, 0.0, 0.0],
        uv_rect: [0.0, 0.0, 0.125, 0.125],
        dimensions: [0.2, 4.0],
        first_particle: 0,
        particle_count: 925,
        kind: 1,
        _pad: [0; 3],
    };
    let grid = |height: i32| {
        let mut grid = OcclusionGrid::default();
        grid.origin = [-32, -32];
        grid.heights.fill(height);
        grid
    };
    let mut mesh = Mesh::default();
    weather_quads(
        &mut mesh,
        &[layer],
        [0.0; 3],
        &grid(render_model::OCCLUSION_OPEN),
        Vec3::ZERO,
    );
    assert_eq!(mesh.indices.len(), 925 * 6, "every streak in the open");
    let mut covered = Mesh::default();
    weather_quads(&mut covered, &[layer], [0.0; 3], &grid(1000), Vec3::ZERO);
    assert!(covered.indices.is_empty(), "nothing falls under a roof");
    let ground = grid(0);
    let mut clipped = Mesh::default();
    weather_quads(&mut clipped, &[layer], [0.0; 3], &ground, Vec3::ZERO);
    assert!(
        positions(&clipped).iter().all(|p| p.y >= 0.5 - 1e-4),
        "streaks end at the surface"
    );
    let rain = PrecipitationLayerRecord { kind: 0, ..layer };
    let mut rain_streaks = Mesh::default();
    weather_quads(
        &mut rain_streaks,
        &[rain],
        [0.0; 3],
        &grid(render_model::OCCLUSION_OPEN),
        Vec3::ZERO,
    );
    assert!(rain_streaks.indices.is_empty(), "rain draws as columns");
}

#[test]
fn rain_falls_in_javas_columns_down_to_the_surface() {
    let sheet = [0.0, 0.125, 0.125, 0.5];
    let grid = |height: i32| {
        let mut grid = OcclusionGrid::default();
        grid.origin = [-32, -32];
        grid.heights.fill(height);
        grid
    };
    let camera = Vec3::new(0.5, 70.5, 0.5);
    let mut blocked = Mesh::default();
    rain_columns(
        &mut blocked,
        &grid(render_model::OCCLUSION_BLOCKED),
        sheet,
        camera,
        1.0,
        37.25,
    );
    assert!(
        blocked.indices.is_empty(),
        "no rain where the biome has none"
    );
    let mut dry = Mesh::default();
    rain_columns(&mut dry, &grid(64), sheet, camera, 0.0, 37.25);
    assert!(dry.indices.is_empty(), "no rain at rain level 0");

    let mut mesh = Mesh::default();
    rain_columns(&mut mesh, &grid(64), sheet, camera, 1.0, 37.25);
    let corners = positions(&mesh);
    let uv = uvs(&mesh);
    assert!(!corners.is_empty());
    // From the surface at y 64 to ten blocks above the camera's block (y 80).
    let lowest = corners.iter().map(|p| p.y).fold(f32::MAX, f32::min);
    let highest = corners.iter().map(|p| p.y).fold(f32::MIN, f32::max);
    assert!((lowest - (64.0 - 70.5)).abs() < 1e-3, "{lowest}");
    assert!((highest - (80.0 - 70.5)).abs() < 1e-3, "{highest}");
    // 21 x 21 columns less the camera's own; every quad is one block wide and faces it.
    let mut columns = std::collections::HashSet::new();
    for quad in corners.chunks(4) {
        let centre = (quad[0] + quad[1]) * 0.5;
        columns.insert((centre.x.round() as i32, centre.z.round() as i32));
        assert!(((quad[1] - quad[0]).length() - 1.0).abs() < 1e-4);
        assert!(
            (quad[1] - quad[0])
                .dot(Vec3::new(centre.x, 0.0, centre.z))
                .abs()
                < 1e-3
        );
    }
    assert_eq!(columns.len(), 21 * 21 - 1);
    assert!(!columns.contains(&(0, 0)));
    // The sheet repeats every four blocks; each piece stays inside the rain cell.
    assert!(
        uv.iter()
            .all(|t| (0.125 - 1e-4..=0.625 + 1e-4).contains(&t[1]))
    );
    for quad in corners.chunks(4) {
        assert!(quad[0].y - quad[3].y <= 4.0 + 1e-3);
    }
}

fn camera() -> EffectCamera {
    EffectCamera {
        position: Vec3::ZERO,
        model_view: Mat4::IDENTITY,
        focal: [1.0, 1.0],
        screen: [1000, 1000],
    }
}

#[test]
fn lines_widen_to_javas_outline_width_on_screen() {
    assert_eq!(outline_width(1600), 2.5);
    assert_eq!(outline_width(3840), 5.0);
    let quad = widen(
        &camera(),
        Vec3::new(-1.0, 0.0, -10.0),
        Vec3::new(1.0, 0.0, -10.0),
        4.0,
    )
    .expect("in front of the camera");
    // One pixel at depth 10 spans 10 / 500 view units.
    let width = (quad[0] - quad[1]).length();
    assert!((width - 4.0 * 10.0 / 500.0).abs() < 1e-5, "{width}");
    assert!(
        widen(
            &camera(),
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(1.0, 0.0, 2.0),
            2.0
        )
        .is_none(),
        "behind the camera"
    );
}

#[test]
fn a_highlighted_cube_outlines_its_twelve_edges() {
    let corner = |x: f32, y: f32, z: f32| BlockEntityVertex {
        position: [x + 0.5, y + 0.5, z - 5.0],
        ..Default::default()
    };
    let mut highlight = vec![];
    for axis in 0..3 {
        for side in [0.0, 1.0] {
            let point = |a: f32, b: f32| {
                let mut p = [0.0; 3];
                p[axis] = side;
                p[(axis + 1) % 3] = a;
                p[(axis + 2) % 3] = b;
                corner(p[0], p[1], p[2])
            };
            let [a, b, c, d] = [
                point(0.0, 0.0),
                point(1.0, 0.0),
                point(1.0, 1.0),
                point(0.0, 1.0),
            ];
            highlight.extend([a, b, c, a, c, d]);
        }
    }
    let mut mesh = Mesh::default();
    outline(&mut mesh, &camera(), &[], &highlight);
    assert_eq!(mesh.indices.len(), 12 * 6);
    let mut wire = Mesh::default();
    outline(&mut wire, &camera(), &highlight[..4], &highlight);
    assert_eq!(
        wire.indices.len(),
        2 * 6,
        "wire mode draws its endpoint pairs"
    );
}

fn cloud_texture() -> AtmosphereTexture {
    let mut rgba8 = vec![0u8; 256 * 256 * 4];
    rgba8[..4].copy_from_slice(&[255, 255, 255, 255]);
    AtmosphereTexture {
        role: AtmosphereRole::Clouds,
        source_path: "textures/environment/clouds.png".into(),
        source_bytes: 0,
        source_sha256: [0; 32],
        pixels_sha256: [0; 32],
        width: 256,
        height: 256,
        rgba8: rgba8.into_boxed_slice(),
    }
}

#[test]
fn clouds_draw_cuboids_when_fancy_and_a_flat_layer_when_fast() {
    let texture = cloud_texture();
    let frame = |fast| CloudFrame {
        texture: &texture,
        identity: [0; 32],
        scroll: 0.0,
        colour: [1.0, 1.0, 1.0, 0.8],
        fast,
    };
    let camera = DVec3::new(8.0, 100.0, 8.0);
    let mut fancy = Mesh::default();
    cloud_quads(
        &mut fancy,
        &mut CloudCache::default(),
        &frame(false),
        camera,
    );
    assert_eq!(fancy.indices.len(), 5 * 6, "the underside and four sides");
    let heights: Vec<f32> = positions(&fancy).iter().map(|p| p.y + 100.0).collect();
    assert!(heights.iter().all(|y| (192.0..197.0).contains(y)));
    let mut fast = Mesh::default();
    cloud_quads(&mut fast, &mut CloudCache::default(), &frame(true), camera);
    assert_eq!(fast.indices.len(), 2 * 6, "both faces of one flat cell");
    assert!(
        positions(&fast)
            .iter()
            .all(|p| (p.y + 100.0 - 192.33).abs() < 1e-3)
    );
    let colour = fast.words[3].to_le_bytes();
    assert_eq!(colour[3], 204, "texel alpha times the cloud colour's");
}

/// Interpolates a UV from the emitted triangles at a point inside the particle quad.
fn interpolated_uv(mesh: &Mesh, point: Vec3) -> [f32; 2] {
    let positions = positions(mesh);
    let uv = uvs(mesh);
    for triangle in mesh.indices.as_chunks::<3>().0 {
        let [a, b, c] = [
            triangle[0] as usize,
            triangle[1] as usize,
            triangle[2] as usize,
        ];
        let ab = (positions[b] - positions[a]).truncate();
        let ac = (positions[c] - positions[a]).truncate();
        let ap = (point - positions[a]).truncate();
        let determinant = ab.perp_dot(ac);
        if determinant.abs() < 1e-8 {
            continue;
        }
        let wb = ap.perp_dot(ac) / determinant;
        let wc = ab.perp_dot(ap) / determinant;
        let wa = 1. - wb - wc;
        if wa >= -1e-4 && wb >= -1e-4 && wc >= -1e-4 {
            return std::array::from_fn(|axis| {
                uv[a][axis] * wa + uv[b][axis] * wb + uv[c][axis] * wc
            });
        }
    }
    panic!("particle geometry does not cover {point:?}");
}

#[test]
fn particles_extend_source_edges_without_changing_the_interior_uv_slope() {
    let mut particle = instance(false);
    particle.uv = [0.2, 0.1, 0.6, 0.8];
    particle.texture_bounds = [0.4, 0.3, 0.2, 0.4];
    for mirror in [false, true] {
        if mirror {
            particle.uv[0] += particle.uv[2];
            particle.uv[2] = -particle.uv[2];
        }
        let mut mesh = Mesh::default();
        particle_quads(&mut mesh, &[particle], Vec3::ZERO, 256);
        for tx in [0., 0.2, 0.4, 0.6, 0.8, 1.] {
            for ty in [0., 0.2, 0.4, 0.6, 0.8, 1.] {
                let point = Vec3::from_slice(&particle.center_light[..3])
                    + Vec3::from_slice(&particle.axis_x[..3]) * (2. * tx - 1.)
                    + Vec3::from_slice(&particle.axis_y[..3]) * (1. - 2. * ty);
                let uv = interpolated_uv(&mesh, point);
                let expected = [
                    (particle.uv[0] + tx * particle.uv[2])
                        .clamp(0.4 + 0.5 / 256., 0.6 - 0.5 / 256.),
                    (particle.uv[1] + ty * particle.uv[3])
                        .clamp(0.3 + 0.5 / 256., 0.7 - 0.5 / 256.),
                ];
                for axis in 0..2 {
                    assert!(
                        (uv[axis] - expected[axis]).abs() < 0.0001,
                        "UV at {tx},{ty}: {uv:?} != {expected:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn crack_triangles_share_the_original_quad_uv_midpoint() {
    let mut vertices = [BlockEntityVertex::default(); 6];
    for (vertex, uv) in
        vertices
            .iter_mut()
            .zip([[0., 0.], [1., 0.], [1., 1.], [0., 0.], [1., 1.], [0., 1.]])
    {
        vertex.uv = uv;
    }
    let mut mesh = Mesh::default();
    super::lines::cracks(&mut mesh, &vertices, Vec3::ZERO);
    let midpoint: Vec<_> = mesh
        .words
        .as_chunks::<WORDS>()
        .0
        .iter()
        .map(|v| v[5])
        .collect();
    assert!(midpoint.iter().all(|&v| v == 0x8000_8000));
}
