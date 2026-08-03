//! Cached geometry-based first-person item carriers.
//!
//! Sprite items are extruded one texel deep and rendered with nearest-neighbour
//! color and a bounded depth buffer. This follows the item-in-hand contract
//! used by Java/Bedrock renderers without treating the inventory icon as a
//! flat HUD quad.

use render::UiRenderTextureArray;

use super::IconRef;

pub(crate) const SIDE: u32 = 96;
const DEPTH: f32 = 0.075;
const MODEL_SCALE: f32 = 70.0;
const MAIN_ORIGIN: [u32; 2] = [128, 0];
const OFFHAND_ORIGIN: [u32; 2] = [128, SIDE];

#[derive(Clone, Copy)]
struct Vertex {
    position: [f32; 3],
}

impl super::UiPresentationRuntime {
    pub(super) fn set_item_viewmodels(&mut self, main: Option<IconRef>, offhand: Option<IconRef>) {
        if self.held_viewmodel_source == main && self.offhand_viewmodel_source == offhand {
            return;
        }
        let Some(page) = self.player_preview_page else {
            self.held_viewmodel_icon = None;
            self.offhand_viewmodel_icon = None;
            return;
        };
        if self.textures.width < MAIN_ORIGIN[0] + SIDE
            || self.textures.height < OFFHAND_ORIGIN[1] + SIDE
        {
            self.held_viewmodel_icon = None;
            self.offhand_viewmodel_icon = None;
            return;
        }
        let main_raster = main.and_then(|icon| render(&self.textures, icon, false));
        let offhand_raster = offhand.and_then(|icon| render(&self.textures, icon, true));
        let mut rgba8 = self.textures.rgba8.to_vec();
        clear_region(
            &mut rgba8,
            self.textures.width,
            self.textures.height,
            page,
            MAIN_ORIGIN,
        );
        clear_region(
            &mut rgba8,
            self.textures.width,
            self.textures.height,
            page,
            OFFHAND_ORIGIN,
        );
        if let Some(raster) = main_raster.as_deref() {
            copy_region(
                &mut rgba8,
                self.textures.width,
                self.textures.height,
                page,
                MAIN_ORIGIN,
                raster,
            );
        }
        if let Some(raster) = offhand_raster.as_deref() {
            copy_region(
                &mut rgba8,
                self.textures.width,
                self.textures.height,
                page,
                OFFHAND_ORIGIN,
                raster,
            );
        }
        let mut identity = sha2::Sha256::new();
        use sha2::Digest as _;
        identity.update(self.base_texture_identity);
        identity.update(b"cinnabar-dynamic-hud-v3");
        identity.update(&rgba8);
        self.textures = std::sync::Arc::new(UiRenderTextureArray {
            identity: identity.finalize().into(),
            width: self.textures.width,
            height: self.textures.height,
            layers: self.textures.layers,
            rgba8: rgba8.into(),
        });
        self.held_viewmodel_source = main;
        self.offhand_viewmodel_source = offhand;
        self.held_viewmodel_icon = main.map(|_| icon_at(page, MAIN_ORIGIN));
        self.offhand_viewmodel_icon = offhand.map(|_| icon_at(page, OFFHAND_ORIGIN));
    }

    pub(super) const fn item_viewmodel_icons(&self) -> (Option<IconRef>, Option<IconRef>) {
        (self.held_viewmodel_icon, self.offhand_viewmodel_icon)
    }
}

const fn icon_at(page: u16, origin: [u32; 2]) -> IconRef {
    IconRef {
        page,
        uv: [
            origin[0] as u16,
            origin[1] as u16,
            (origin[0] + SIDE) as u16,
            (origin[1] + SIDE) as u16,
        ],
    }
}

fn clear_region(rgba8: &mut [u8], width: u32, height: u32, page: u16, origin: [u32; 2]) {
    let layer_start = usize::from(page) * width as usize * height as usize * 4;
    for row in 0..SIDE as usize {
        let start =
            layer_start + ((origin[1] as usize + row) * width as usize + origin[0] as usize) * 4;
        rgba8[start..start + SIDE as usize * 4].fill(0);
    }
}

fn copy_region(
    rgba8: &mut [u8],
    width: u32,
    height: u32,
    page: u16,
    origin: [u32; 2],
    raster: &[u8],
) {
    let layer_start = usize::from(page) * width as usize * height as usize * 4;
    for row in 0..SIDE as usize {
        let source = row * SIDE as usize * 4;
        let target =
            layer_start + ((origin[1] as usize + row) * width as usize + origin[0] as usize) * 4;
        rgba8[target..target + SIDE as usize * 4]
            .copy_from_slice(&raster[source..source + SIDE as usize * 4]);
    }
}

pub(crate) fn render(
    textures: &UiRenderTextureArray,
    icon: IconRef,
    left_hand: bool,
) -> Option<Vec<u8>> {
    let width = usize::from(icon.uv[2].checked_sub(icon.uv[0])?);
    let height = usize::from(icon.uv[3].checked_sub(icon.uv[1])?);
    if width == 0 || height == 0 || u32::from(icon.page) >= textures.layers {
        return None;
    }
    let mut source = vec![[0u8; 4]; width.checked_mul(height)?];
    let layer_stride = usize::try_from(textures.width)
        .ok()?
        .checked_mul(textures.height as usize)?
        .checked_mul(4)?;
    let layer_start = usize::from(icon.page).checked_mul(layer_stride)?;
    for y in 0..height {
        for x in 0..width {
            let offset = layer_start
                + ((usize::from(icon.uv[1]) + y) * textures.width as usize
                    + usize::from(icon.uv[0])
                    + x)
                    * 4;
            source[y * width + x] = textures.rgba8.get(offset..offset + 4)?.try_into().ok()?;
        }
    }

    let side = SIDE as usize;
    let mut pixels = vec![0u8; side * side * 4];
    let mut depth = vec![f32::NEG_INFINITY; side * side];
    for y in 0..height {
        for x in 0..width {
            let color = source[y * width + x];
            if color[3] < 8 {
                continue;
            }
            let x0 = x as f32 / width as f32 - 0.5;
            let x1 = (x + 1) as f32 / width as f32 - 0.5;
            let y0 = 0.5 - (y + 1) as f32 / height as f32;
            let y1 = 0.5 - y as f32 / height as f32;
            let front = [
                vertex(x0, y0, DEPTH, left_hand),
                vertex(x1, y0, DEPTH, left_hand),
                vertex(x1, y1, DEPTH, left_hand),
                vertex(x0, y1, DEPTH, left_hand),
            ];
            quad(&mut pixels, &mut depth, front, color, 1.0);

            if transparent(&source, width, height, x as isize - 1, y as isize) {
                side_quad(
                    &mut pixels,
                    &mut depth,
                    [x0, y0],
                    [x0, y1],
                    color,
                    0.62,
                    left_hand,
                );
            }
            if transparent(&source, width, height, x as isize + 1, y as isize) {
                side_quad(
                    &mut pixels,
                    &mut depth,
                    [x1, y1],
                    [x1, y0],
                    color,
                    0.78,
                    left_hand,
                );
            }
            if transparent(&source, width, height, x as isize, y as isize - 1) {
                side_quad(
                    &mut pixels,
                    &mut depth,
                    [x0, y1],
                    [x1, y1],
                    color,
                    0.9,
                    left_hand,
                );
            }
            if transparent(&source, width, height, x as isize, y as isize + 1) {
                side_quad(
                    &mut pixels,
                    &mut depth,
                    [x1, y0],
                    [x0, y0],
                    color,
                    0.7,
                    left_hand,
                );
            }
        }
    }
    Some(pixels)
}

fn transparent(source: &[[u8; 4]], width: usize, height: usize, x: isize, y: isize) -> bool {
    x < 0
        || y < 0
        || x as usize >= width
        || y as usize >= height
        || source[y as usize * width + x as usize][3] < 8
}

fn side_quad(
    pixels: &mut [u8],
    depth: &mut [f32],
    a: [f32; 2],
    b: [f32; 2],
    color: [u8; 4],
    shade: f32,
    left_hand: bool,
) {
    quad(
        pixels,
        depth,
        [
            vertex(a[0], a[1], DEPTH, left_hand),
            vertex(b[0], b[1], DEPTH, left_hand),
            vertex(b[0], b[1], -DEPTH, left_hand),
            vertex(a[0], a[1], -DEPTH, left_hand),
        ],
        color,
        shade,
    );
}

fn vertex(mut x: f32, mut y: f32, mut z: f32, left_hand: bool) -> Vertex {
    let handed: f32 = if left_hand { -1.0 } else { 1.0 };
    let (sin_y, cos_y) = (handed * 0.58).sin_cos();
    (x, z) = (x * cos_y - z * sin_y, x * sin_y + z * cos_y);
    let (sin_x, cos_x) = (-0.30_f32).sin_cos();
    (y, z) = (y * cos_x - z * sin_x, y * sin_x + z * cos_x);
    let (sin_z, cos_z) = (handed * 0.22).sin_cos();
    (x, y) = (x * cos_z - y * sin_z, x * sin_z + y * cos_z);
    Vertex {
        position: [
            SIDE as f32 * 0.5 + x * MODEL_SCALE,
            SIDE as f32 * 0.5 - y * MODEL_SCALE,
            z,
        ],
    }
}

fn quad(
    pixels: &mut [u8],
    depth: &mut [f32],
    vertices: [Vertex; 4],
    mut color: [u8; 4],
    shade: f32,
) {
    for channel in &mut color[..3] {
        *channel = (f32::from(*channel) * shade).round().clamp(0.0, 255.0) as u8;
    }
    triangle(
        pixels,
        depth,
        [vertices[0], vertices[1], vertices[2]],
        color,
    );
    triangle(
        pixels,
        depth,
        [vertices[0], vertices[2], vertices[3]],
        color,
    );
}

fn triangle(pixels: &mut [u8], depth: &mut [f32], vertices: [Vertex; 3], color: [u8; 4]) {
    let points = vertices.map(|vertex| [vertex.position[0], vertex.position[1]]);
    let area = edge(points[0], points[1], points[2]);
    if area.abs() < f32::EPSILON {
        return;
    }
    let min_x = points
        .iter()
        .map(|point| point[0])
        .fold(f32::INFINITY, f32::min)
        .floor()
        .clamp(0.0, SIDE as f32 - 1.0) as usize;
    let max_x = points
        .iter()
        .map(|point| point[0])
        .fold(f32::NEG_INFINITY, f32::max)
        .ceil()
        .clamp(0.0, SIDE as f32 - 1.0) as usize;
    let min_y = points
        .iter()
        .map(|point| point[1])
        .fold(f32::INFINITY, f32::min)
        .floor()
        .clamp(0.0, SIDE as f32 - 1.0) as usize;
    let max_y = points
        .iter()
        .map(|point| point[1])
        .fold(f32::NEG_INFINITY, f32::max)
        .ceil()
        .clamp(0.0, SIDE as f32 - 1.0) as usize;
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let point = [x as f32 + 0.5, y as f32 + 0.5];
            let weights = [
                edge(points[1], points[2], point) / area,
                edge(points[2], points[0], point) / area,
                edge(points[0], points[1], point) / area,
            ];
            if weights.iter().any(|weight| *weight < -0.0001) {
                continue;
            }
            let z = weights[0] * vertices[0].position[2]
                + weights[1] * vertices[1].position[2]
                + weights[2] * vertices[2].position[2];
            let index = y * SIDE as usize + x;
            if z >= depth[index] {
                depth[index] = z;
                pixels[index * 4..index * 4 + 4].copy_from_slice(&color);
            }
        }
    }
}

fn edge(a: [f32; 2], b: [f32; 2], point: [f32; 2]) -> f32 {
    (point[0] - a[0]) * (b[1] - a[1]) - (point[1] - a[1]) * (b[0] - a[0])
}
