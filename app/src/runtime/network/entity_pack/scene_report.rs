//! Env-gated offline frame of a captured lobby: pack entities and every name tag, drawn from one
//! camera with the same billboard maths as `nametag.wgsl`.

use std::{path::Path, sync::Arc};

use bevy::math::{EulerRot, Mat4, Quat, Vec3, Vec4};
use protocol::{
    ActorEvent, ActorKind, ActorMetadata, ActorMetadataValue, ActorSpawnEvent, WorldEvent,
};
use render::{ActorArtworkPages, ActorRenderScene};
use render_model::{NAMETAG_ATLAS_SIDE, NametagScene};
use ui::TextLayoutCache;

use super::render_report::{compile_local_pack, world_for};
use client_presentation::presentation::{actors, entity_layers};
use client_ui::ui_runtime::presentation::{
    nametag_atlas::{GlyphPage, GlyphPixels, NametagAtlas, font_page},
    nametags::{build_nametag_scene, extract_nametag},
};

pub(super) const WIDTH: u32 = 1280;
pub(super) const HEIGHT: u32 = 752;
const HORIZONTAL_FOV_DEGREES: f32 = 90.0;

/// `CINNABAR_RENDER_SCENE` is a capture TSV (`E|P, identifier|username, variant, scale, x, y, z,
/// width, height, name, flags, always_show, score`), `CINNABAR_RENDER_CAMERA` is
/// `x,y,z,yaw,pitch` (radians, Bevy convention), `CINNABAR_RENDER_FONT` a `.mcbefont`,
/// `CINNABAR_RENDER_GLYPHS` a directory of `glyph_XX.png`; the frame goes to `CINNABAR_RENDER_OUT`.
#[test]
fn render_captured_scene() {
    let vars = [
        "CINNABAR_RENDER_PACK",
        "CINNABAR_RENDER_SCENE",
        "CINNABAR_RENDER_CAMERA",
        "CINNABAR_RENDER_FONT",
        "CINNABAR_RENDER_GLYPHS",
        "CINNABAR_RENDER_OUT",
    ]
    .map(|name| match std::env::var(name) {
        Ok(value) => Some(value),
        Err(std::env::VarError::NotPresent) => {
            eprintln!("skipping captured-scene fixture test: {name} is not set");
            None
        }
        Err(error) => panic!("read scene fixture setting {name}: {error}"),
    });
    let [
        Some(pack),
        Some(scene),
        Some(camera),
        Some(font),
        Some(glyphs),
        Some(out),
    ] = vars
    else {
        return;
    };
    for (name, path) in [
        ("CINNABAR_RENDER_PACK", &pack),
        ("CINNABAR_RENDER_SCENE", &scene),
        ("CINNABAR_RENDER_FONT", &font),
        ("CINNABAR_RENDER_GLYPHS", &glyphs),
    ] {
        if !Path::new(path).exists() {
            eprintln!("skipping captured-scene fixture test: missing {name} fixture {path}");
            return;
        }
    }
    let pack = compile_local_pack(Path::new(&pack));
    let camera: Vec<f32> = camera
        .split(',')
        .map(|value| value.parse().unwrap())
        .collect();
    let eye = Vec3::new(camera[0], camera[1], camera[2]);
    let mut world = world_for(&pack.entities, &pack.candidates, eye.to_array());
    let mut runtime_ids = Vec::new();
    for (index, row) in std::fs::read_to_string(scene).unwrap().lines().enumerate() {
        let runtime_id = 100 + index as u64;
        if let Some(event) = spawn_event(row, runtime_id) {
            world.submit(index as u64 + 1, event).unwrap();
            runtime_ids.push(runtime_id);
        }
    }
    world.advance_actor_interpolation_ticks(20);
    let rotation = Quat::from_euler(EulerRot::YXZ, camera[3], camera[4], 0.0);
    let aspect = WIDTH as f32 / HEIGHT as f32;
    let vertical_fov = 2.0 * ((HORIZONTAL_FOV_DEGREES.to_radians() / 2.0).tan() / aspect).atan();
    let clip_from_world = Mat4::perspective_rh(vertical_fov, aspect, 0.05, 500.0)
        * Mat4::from_rotation_translation(rotation, eye).inverse();
    let mut frame = Frame::new(clip_from_world);
    draw_actors(
        &mut frame,
        &world,
        &runtime_ids,
        &pack.entities,
        &pack.artwork,
    );
    let (font, glyph_pages) = font_with_glyphs(Path::new(&font), Path::new(&glyphs));
    let anchors: Vec<_> = runtime_ids
        .iter()
        .filter_map(|id| world.authority().actor(*id))
        .filter_map(|actor| {
            extract_nametag(
                actor,
                eye,
                None,
                world.authority().actor_name_tag(actor.unique_id)?,
                &ui::ScoreboardStore::default(),
                1.0,
            )
        })
        .collect();
    let first_glyph_page = font.pages().len();
    let scene = build_nametag_scene(
        &anchors,
        &font,
        &mut TextLayoutCache::new(64, 1 << 24),
        &mut NametagAtlas::default(),
        &|page| {
            font_page(&font, page).or_else(|| {
                glyph_pages
                    .get(page.checked_sub(first_glyph_page)?)
                    .map(|pixels| GlyphPage {
                        width: GLYPH_PAGE_SIDE,
                        height: GLYPH_PAGE_SIDE,
                        pixels: GlyphPixels::Rgba8(pixels),
                    })
            })
        },
    );
    draw_nametags(&mut frame, &scene, eye);
    eprintln!(
        "scene: {} tags, {} records",
        anchors.len(),
        scene.records.len()
    );
    frame.image.save(out).unwrap();
}

const GLYPH_PAGE_SIDE: u32 = 256;

fn font_with_glyphs(font: &Path, glyphs: &Path) -> (assets::RuntimeFontCatalog, Vec<Box<[u8]>>) {
    let manifest = crate::asset_startup::canonical_source_manifest_sha256(include_str!(
        "../../../../../assets/cinnangles-sans-source.json"
    ));
    let base = assets::RuntimeFontCatalog::decode(&std::fs::read(font).unwrap(), manifest).unwrap();
    let mut cells = Vec::new();
    for high_byte in 0..=u8::MAX {
        let Ok(image) = image::open(glyphs.join(format!("glyph_{high_byte:02X}.png"))) else {
            continue;
        };
        let image = image.into_rgba8();
        cells.extend(assets::extract_cells(&assets::GlyphSheet {
            high_byte,
            width: image.width(),
            height: image.height(),
            rgba8: image.into_raw().into_boxed_slice(),
        }));
    }
    let atlas = assets::pack_cells(&cells, base.pages().len() as u16, GLYPH_PAGE_SIDE, 64);
    let font = base.with_glyphs(&atlas.glyphs, |c| ('\u{e000}'..='\u{f8ff}').contains(&c));
    (font, atlas.pages)
}

fn spawn_event(row: &str, runtime_id: u64) -> Option<WorldEvent> {
    let fields: Vec<&str> = row.split('\t').collect();
    if fields.len() < 13 {
        return None;
    }
    let number = |index: usize| fields.get(index)?.parse::<f32>().ok();
    let text = |index: usize| serde_json::from_str::<String>(fields.get(index)?).ok();
    let kind = match fields.first()? {
        &"P" => ActorKind::Player {
            uuid: runtime_id.to_le_bytes().repeat(2).try_into().ok()?,
            username: Arc::from(fields[1]),
        },
        _ => ActorKind::Entity {
            identifier: Arc::from(fields[1]),
        },
    };
    let mut metadata = vec![
        ActorMetadata {
            key: 0,
            value: ActorMetadataValue::Flags(fields.get(10)?.parse().unwrap_or(0)),
        },
        ActorMetadata {
            key: 2,
            value: ActorMetadataValue::Int(fields.get(2)?.parse().unwrap_or(0)),
        },
        ActorMetadata {
            key: 38,
            value: ActorMetadataValue::Float(number(3).unwrap_or(1.0)),
        },
        ActorMetadata {
            key: 81,
            value: ActorMetadataValue::Byte(fields.get(11)?.parse().unwrap_or(0)),
        },
    ];
    for (key, index) in [(53, 7), (54, 8)] {
        if let Some(value) = number(index).filter(|value| *value > 0.0) {
            metadata.push(ActorMetadata {
                key,
                value: ActorMetadataValue::Float(value),
            });
        }
    }
    for (key, index) in [(4, 9), (84, 12)] {
        if let Some(value) = text(index).filter(|value| !value.is_empty()) {
            metadata.push(ActorMetadata {
                key,
                value: ActorMetadataValue::String(value.into()),
            });
        }
    }
    Some(WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
        dimension: 0,
        unique_id: -(runtime_id as i64),
        runtime_id,
        kind,
        position: [number(4)?, number(5)?, number(6)?],
        velocity: [0.0; 3],
        pitch: 0.0,
        yaw: 0.0,
        head_yaw: 0.0,
        body_yaw: 0.0,
        held_item: Default::default(),
        metadata: metadata.into(),
        attributes: Arc::from([]),
        properties: Arc::from([]),
        links: Arc::from([]),
    })))
}

pub(super) struct Frame {
    pub(super) image: image::RgbaImage,
    depth: Vec<f32>,
    clip_from_world: Mat4,
}

/// A projected vertex: window px, depth in `0..1` and the clip `w` for perspective correction.
#[derive(Clone, Copy)]
struct Projected {
    x: f32,
    y: f32,
    z: f32,
    w: f32,
}

impl Frame {
    pub(super) fn new(clip_from_world: Mat4) -> Self {
        Self {
            image: image::RgbaImage::from_pixel(WIDTH, HEIGHT, image::Rgba([138, 178, 232, 255])),
            depth: vec![f32::INFINITY; (WIDTH * HEIGHT) as usize],
            clip_from_world,
        }
    }

    fn project(&self, point: Vec3) -> Option<Projected> {
        let clip: Vec4 = self.clip_from_world * point.extend(1.0);
        if clip.w <= 0.05 {
            return None;
        }
        Some(Projected {
            x: (clip.x / clip.w * 0.5 + 0.5) * WIDTH as f32,
            y: (0.5 - clip.y / clip.w * 0.5) * HEIGHT as f32,
            z: clip.z / clip.w,
            w: clip.w,
        })
    }

    /// Fills a triangle; `shade` returns straight-alpha RGBA for perspective-correct `uv`.
    pub(super) fn triangle(
        &mut self,
        corners: [(Vec3, [f32; 2]); 3],
        depth_test: bool,
        depth_write: bool,
        shade: &impl Fn([f32; 2]) -> Option<[u8; 4]>,
    ) {
        let Some(points) = corners
            .iter()
            .map(|(point, _)| self.project(*point))
            .collect::<Option<Vec<_>>>()
        else {
            return;
        };
        let [a, b, c] = [points[0], points[1], points[2]];
        let area = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
        if area.abs() < 1e-6 {
            return;
        }
        let min_x = a.x.min(b.x).min(c.x).floor().max(0.0) as u32;
        let max_x = a.x.max(b.x).max(c.x).ceil().min(WIDTH as f32 - 1.0) as u32;
        let min_y = a.y.min(b.y).min(c.y).floor().max(0.0) as u32;
        let max_y = a.y.max(b.y).max(c.y).ceil().min(HEIGHT as f32 - 1.0) as u32;
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let p = [x as f32 + 0.5, y as f32 + 0.5];
                let w0 = ((b.x - p[0]) * (c.y - p[1]) - (b.y - p[1]) * (c.x - p[0])) / area;
                let w1 = ((c.x - p[0]) * (a.y - p[1]) - (c.y - p[1]) * (a.x - p[0])) / area;
                let w2 = 1.0 - w0 - w1;
                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                    continue;
                }
                let z = w0 * a.z + w1 * b.z + w2 * c.z;
                let slot = (y * WIDTH + x) as usize;
                if depth_test && z > self.depth[slot] {
                    continue;
                }
                let inverse = [w0 / a.w, w1 / b.w, w2 / c.w];
                let total = inverse.iter().sum::<f32>();
                let uv = std::array::from_fn(|axis| {
                    (0..3).map(|i| inverse[i] * corners[i].1[axis]).sum::<f32>() / total
                });
                let Some(color) = shade(uv) else {
                    continue;
                };
                if depth_write {
                    self.depth[slot] = z;
                }
                let alpha = f32::from(color[3]) / 255.0;
                let pixel = self.image.get_pixel_mut(x, y);
                for channel in 0..3 {
                    pixel[channel] = (f32::from(color[channel]) * alpha
                        + f32::from(pixel[channel]) * (1.0 - alpha))
                        .round() as u8;
                }
            }
        }
    }
}

pub(super) fn draw_actors(
    frame: &mut Frame,
    world: &chunk_pipeline::WorldStream,
    runtime_ids: &[u64],
    entities: &assets::RuntimeEntityAssets,
    artwork: &ActorArtworkPages,
) {
    let bodies: Vec<_> = runtime_ids
        .iter()
        .filter_map(|id| {
            let rig = world.authority().actor_rig(*id)?;
            client_presentation::presentation::actors::entity_rig_presentation(
                &rig,
                world.authority().actor(*id)?,
                artwork,
                1.0,
            )
        })
        .collect();
    let mut batch = client_presentation::presentation::actors::select_actor_presentations(
        1, false, None, bodies,
    );
    client_presentation::presentation::entity_layers::apply_render_layers(
        &mut batch,
        |id| world.authority().actor_rig(id),
        artwork,
    );
    let mut scene = ActorRenderScene::default();
    scene.replace_pack_entities(Some(entities)).unwrap();
    scene.configure_artwork(artwork.clone());
    let rendered =
        scene.update_rigs_with_artwork(1.0, None, batch.submissions.clone(), &[], &batch.artwork);
    let rig = rendered.rig.clone();
    for (instance, entry) in rig.instances.iter().zip(rig.manifest.iter()) {
        let Some(location) = batch.artwork.get(&entry.identity) else {
            continue;
        };
        let Some(page) = usize::from(location.page())
            .checked_sub(1)
            .and_then(|page| artwork.pages().get(page))
        else {
            continue;
        };
        let (width, height) = page.dimensions();
        let layer = location.layer() as usize;
        let uv_anim = instance.uv_anim;
        let shade = |uv: [f32; 2]| {
            let [u, v] = std::array::from_fn(|axis| {
                let animated = uv_anim[axis] + uv[axis] * uv_anim[axis + 2];
                if uv_anim == render::IDENTITY_UV_ANIM {
                    animated.clamp(0.0, 1.0)
                } else {
                    animated.rem_euclid(1.0)
                }
            });
            let tx = ((u * f32::from(width)) as usize).min(usize::from(width) - 1);
            let ty = ((v * f32::from(height)) as usize).min(usize::from(height) - 1);
            let at = ((layer * usize::from(height) + ty) * usize::from(width) + tx) * 4;
            let texel = &page.pixels()[at..at + 4];
            (texel[3] >= 26).then(|| [texel[0], texel[1], texel[2], 255])
        };
        let span = rig.geometry_spans[instance.geometry_id as usize];
        let Some(vertices) = rig.geometry_vertices.span(span) else {
            continue;
        };
        for corners in vertices.chunks_exact(3) {
            let placed = std::array::from_fn(|corner| {
                let vertex = corners[corner];
                let bone =
                    rig.current_bones[(instance.current_bone_base + vertex.bone_index) as usize];
                let posed = apply(&bone, vertex.position);
                (
                    Vec3::from_array(apply(&instance.world_from_actor, posed)),
                    vertex.uv,
                )
            });
            let points = placed.map(|(point, _)| frame.project(point));
            let [Some(a), Some(b), Some(c)] = points else {
                continue;
            };
            let area = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
            if area >= 0.0 {
                if corners[0].back_uv[0] < -1.0e8 {
                    continue;
                }
                frame.triangle(
                    std::array::from_fn(|i| (placed[i].0, corners[i].back_uv)),
                    true,
                    true,
                    &shade,
                );
            } else {
                frame.triangle(placed, true, true, &shade);
            }
        }
    }
}

fn apply(rows: &[[f32; 4]; 3], point: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|axis| {
        rows[axis][0] * point[0]
            + rows[axis][1] * point[1]
            + rows[axis][2] * point[2]
            + rows[axis][3]
    })
}

/// The shader's billboard: facing the camera position by yaw then pitch, x right and y down.
fn draw_nametags(frame: &mut Frame, scene: &NametagScene, eye: Vec3) {
    let side = NAMETAG_ATLAS_SIDE as f32;
    let atlas = nametag_pixels(scene);
    for (index, record) in scene.records.iter().enumerate() {
        let Some(corners) = record.world_corners(eye.to_array()) else {
            continue;
        };
        let [u0, v0, u1, v1] = record.uv;
        let quad = [
            (Vec3::from_array(corners[0]), [u0, v0]),
            (Vec3::from_array(corners[1]), [u1, v0]),
            (Vec3::from_array(corners[2]), [u1, v1]),
            (Vec3::from_array(corners[3]), [u0, v1]),
        ];
        let color = record.color;
        let shade = |uv: [f32; 2]| {
            let mut rgba = color;
            if u1 >= 0.0 {
                let tx = ((uv[0] * side) as usize).min(NAMETAG_ATLAS_SIDE as usize - 1);
                let ty = ((uv[1] * side) as usize).min(NAMETAG_ATLAS_SIDE as usize - 1);
                let at = (ty * NAMETAG_ATLAS_SIDE as usize + tx) * 4;
                let texel = &atlas[at..at + 4];
                if index >= scene.see_through && record.text != 0 && texel[3] < 128 {
                    return None;
                }
                for (channel, value) in rgba.iter_mut().zip(texel) {
                    *channel *= f32::from(*value) / 255.0;
                }
            }
            (rgba[3] > 0.0).then(|| rgba.map(|value| (value.clamp(0.0, 1.0) * 255.0).round() as u8))
        };
        let depth_tested = index >= scene.see_through;
        frame.triangle(
            [quad[0], quad[1], quad[2]],
            depth_tested,
            record.text != 0,
            &shade,
        );
        frame.triangle(
            [quad[0], quad[2], quad[3]],
            depth_tested,
            record.text != 0,
            &shade,
        );
    }
}

/// Reconstructs the atlas for this offline software-rendered frame.
fn nametag_pixels(scene: &NametagScene) -> Vec<u8> {
    let side = NAMETAG_ATLAS_SIDE as usize;
    let mut pixels = vec![0; side * side * 4];
    for rectangle in scene.atlas.iter() {
        let [x, y, width, height] = rectangle.cell.map(|value| value as usize);
        for row in 0..height {
            let source = row * width * 4;
            let target = ((y + row) * side + x) * 4;
            pixels[target..target + width * 4]
                .copy_from_slice(&rectangle.rgba8[source..source + width * 4]);
        }
    }
    pixels
}
