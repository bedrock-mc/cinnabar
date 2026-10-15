//! The open book on enchanting tables and lecterns, and the lectern stand.
//!
//! A table's book turns toward the nearest player within three blocks, opens while someone
//! is near and leafs toward random pages, matching Bedrock 1.26.10. The lectern stand
//! placement still needs native measurement.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use assets::block_entity_geometry::{self as geometry, BoxSpec};
use glam::{Mat4, Vec3};

use crate::celestial::lerp;

use super::{
    atlas::BlockEntityAtlas,
    mesh::{Layer, MeshBuilder, WHITE, model_matrix},
};

/// Players farther than this from the book's centre leave it closing.
const READER_RANGE_BLOCKS: f32 = 3.0;
/// The book's centre, where readers are measured from, relative to the table's corner.
const CENTRE_OFFSET: Vec3 = Vec3::new(0.5, 1.0, 0.5);
/// Openness gained or lost per tick.
const OPEN_STEP: f32 = 0.1;
/// Radians the book drifts per tick with nobody near.
const IDLE_TURN: f32 = 0.02;
/// Share of the remaining turn toward its target the book makes each tick.
const TURN_EASE: f32 = 0.4;
/// While open, one tick in this many picks a new page target.
const NEW_PAGE_TARGET_ODDS: u32 = 40;
/// Page speed per page still to turn, its cap in pages per tick, and how fast it responds.
const FLIP_GAIN: f32 = 0.4;
const FLIP_MAX_SPEED: f32 = 0.2;
const FLIP_RESPONSE: f32 = 0.9;
/// The spine's height above the table's base, and its bob.
const HOVER_BLOCKS: f32 = 0.85;
const BOB_BLOCKS: f32 = 0.01;
const BOB_RATE: f32 = 0.1;
/// How far the book leans back from upright, toward the reader.
const LEAN_RADIANS: f32 = 80.0 * PI / 180.0;
/// Each cover's angle from closed when fully open, and its slow wobble.
const COVER_OPEN_RADIANS: f32 = 1.25;
const COVER_WOBBLE_RADIANS: f32 = 0.1;
const COVER_WOBBLE_RATE: f32 = 0.02;
/// Pixels between the front and back faces of the flat covers, seam and loose pages.
const THIN: f32 = 0.01;

/// Wraps an angle into `[-PI, PI)`.
fn wrap_radians(angle: f32) -> f32 {
    (angle + PI).rem_euclid(TAU) - PI
}

/// The animated values of a book at one tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct BookState {
    /// Facing about the vertical axis in radians; the open pages face `(cos, sin)` in X/Z.
    rot: f32,
    /// Openness from closed (0) to fully open (1).
    open: f32,
    /// Pages turned so far, fractional while a page is moving.
    flip: f32,
}

/// One enchanting table's book. It turns toward the nearest reader, opens while someone is
/// near and leafs toward random pages, advancing once per game tick.
#[derive(Clone, Debug, PartialEq)]
pub struct EnchantingBook {
    centre: Vec3,
    previous: BookState,
    current: BookState,
    ticks: u32,
    rot_target: f32,
    flip_target: f32,
    flip_speed: f32,
    random: u64,
}

/// Where a book stands between two ticks, ready to draw.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BookPose {
    /// Ticks lived, including the partial tick; drives the bob and cover wobble.
    pub time: f32,
    /// Facing about the vertical axis in radians; the open pages face `(cos, sin)` in X/Z.
    pub rot: f32,
    /// Openness from closed (0) to fully open (1).
    pub open: f32,
    /// Pages turned so far.
    pub flip: f32,
}

impl EnchantingBook {
    /// A closed book on the table at `block`; its page choices are seeded by the position.
    #[must_use]
    pub fn new(block: [i32; 3]) -> Self {
        let [x, y, z] = block.map(|axis| axis as u64);
        Self {
            centre: Vec3::from_array(block.map(|axis| axis as f32)) + CENTRE_OFFSET,
            previous: BookState::default(),
            current: BookState::default(),
            ticks: 0,
            rot_target: 0.0,
            flip_target: 0.0,
            flip_speed: 0.0,
            random: (x.wrapping_mul(0x9E37_79B9_7F4A_7C15)
                ^ y.wrapping_mul(0xBF58_476D_1CE4_E5B9)
                ^ z.wrapping_mul(0x94D0_49BB_1331_11EB))
                | 1,
        }
    }

    /// The nearest of `players` (eye-level positions) close enough to read this book.
    #[must_use]
    pub fn nearest_reader(&self, players: impl IntoIterator<Item = Vec3>) -> Option<Vec3> {
        players
            .into_iter()
            .map(|player| (player.distance_squared(self.centre), player))
            .filter(|(distance, _)| *distance < READER_RANGE_BLOCKS * READER_RANGE_BLOCKS)
            .min_by(|left, right| left.0.total_cmp(&right.0))
            .map(|(_, player)| player)
    }

    /// Advances one game tick with `reader` as the nearest reader, if any.
    pub fn tick(&mut self, reader: Option<Vec3>) {
        self.previous = self.current;
        if let Some(reader) = reader {
            let toward = reader - self.centre;
            self.rot_target = toward.z.atan2(toward.x);
            self.current.open += OPEN_STEP;
            if self.current.open < 0.5 || self.roll(NEW_PAGE_TARGET_ODDS) == 0 {
                self.flip_target += self.page_step();
            }
        } else {
            self.rot_target += IDLE_TURN;
            self.current.open -= OPEN_STEP;
        }
        let state = &mut self.current;
        state.rot = wrap_radians(state.rot);
        state.rot += wrap_radians(self.rot_target - state.rot) * TURN_EASE;
        state.open = state.open.clamp(0.0, 1.0);
        let wanted =
            ((self.flip_target - state.flip) * FLIP_GAIN).clamp(-FLIP_MAX_SPEED, FLIP_MAX_SPEED);
        self.flip_speed += (wanted - self.flip_speed) * FLIP_RESPONSE;
        state.flip += self.flip_speed;
        self.ticks = self.ticks.wrapping_add(1);
    }

    /// The pose `partial` of the way from the previous tick to the current one.
    #[must_use]
    pub fn pose(&self, partial: f32) -> BookPose {
        let (from, to) = (self.previous, self.current);
        BookPose {
            time: self.ticks as f32 + partial,
            rot: from.rot + wrap_radians(to.rot - from.rot) * partial,
            open: lerp(from.open, to.open, partial),
            flip: lerp(from.flip, to.flip, partial),
        }
    }

    /// A nonzero page jump of up to three pages either way, small jumps likeliest.
    fn page_step(&mut self) -> f32 {
        loop {
            let step = self.roll(4) as i32 - self.roll(4) as i32;
            if step != 0 {
                return step as f32;
            }
        }
    }

    /// A uniform integer below `bound` from the book's xorshift stream.
    fn roll(&mut self, bound: u32) -> u32 {
        self.random ^= self.random << 13;
        self.random ^= self.random >> 7;
        self.random ^= self.random << 17;
        ((self.random >> 32) % u64::from(bound)) as u32
    }
}

/// How far each of the two loose pages is through its turn after `flip` page turns, from
/// resting on the right pages (0) to resting on the left (1).
fn page_turns(flip: f32) -> [f32; 2] {
    [0.25, 0.75].map(|offset| ((flip + offset).rem_euclid(1.0) * 1.6 - 0.3).clamp(0.0, 1.0))
}

/// The six moving parts' turn about the spine and offset, in model pixels, in the order
/// left cover, right cover, left pages, right pages, first and second loose page.
type PartPoses = [(f32, [f32; 3]); 6];

/// Part poses for an enchanting-table book: each cover opens `open` from closed and the
/// page blocks follow it.
fn table_parts(pose: BookPose) -> PartPoses {
    let open = (COVER_OPEN_RADIANS + COVER_WOBBLE_RADIANS * (pose.time * COVER_WOBBLE_RATE).sin())
        * pose.open;
    let [first, second] = page_turns(pose.flip);
    let pages = [open.sin(), 0.0, 0.0];
    [
        (PI + open, [0.0; 3]),
        (-open, [0.0; 3]),
        (open, pages),
        (-open, pages),
        (open - 2.0 * open * first, pages),
        (open - 2.0 * open * second, pages),
    ]
}

/// The lectern book lies nearly flat with its loose pages lifted.
const LECTERN_COVER_RADIANS: f32 = FRAC_PI_2 - 0.15;
const LECTERN_LOOSE_PAGE_RADIANS: f32 = FRAC_PI_2 - 0.45;
const LECTERN_PARTS: PartPoses = [
    (PI + LECTERN_COVER_RADIANS, [0.0; 3]),
    (-LECTERN_COVER_RADIANS, [0.0; 3]),
    (PI + FRAC_PI_2 + 0.15, [0.02, 0.0, 0.15]),
    (LECTERN_COVER_RADIANS, [0.02, 0.0, -0.15]),
    (LECTERN_LOOSE_PAGE_RADIANS, [1.0, 0.0, 0.0]),
    (-LECTERN_LOOSE_PAGE_RADIANS, [1.0, 0.0, 0.0]),
];

/// Emits the book model in `matrix` space (pixels): the spine runs along Y, the closed book
/// lies on +X, and the open pages face +X.
fn emit_book(builder: &mut MeshBuilder, atlas: &BlockEntityAtlas, matrix: Mat4, parts: PartPoses) {
    let Some(texture) = atlas.texture(
        assets::block_entity_geometry::textures::AtlasTexture::EntityEnchantingTableBook.path(),
        [64.0, 32.0],
    ) else {
        return;
    };
    let pivots = [
        [0.0, 0.0, -1.0],
        [0.0, 0.0, 1.0],
        [0.0; 3],
        [0.0; 3],
        [0.0; 3],
        [0.0; 3],
    ];
    let boxes = [
        BoxSpec::new([-6.0, -5.0, 0.0], [6.0, 10.0, 0.0], [0.0, 0.0]),
        BoxSpec::new([0.0, -5.0, 0.0], [6.0, 10.0, 0.0], [16.0, 0.0]),
        BoxSpec::new([0.0, -4.0, -0.99], [5.0, 8.0, 1.0], [0.0, 10.0]),
        BoxSpec::new([0.0, -4.0, -0.01], [5.0, 8.0, 1.0], [12.0, 10.0]),
        BoxSpec::new([0.0, -4.0, 0.0], [5.0, 8.0, 0.0], [24.0, 10.0]),
        BoxSpec::new([0.0, -4.0, 0.0], [5.0, 8.0, 0.0], [24.0, 10.0]),
    ];
    for (((turn, offset), pivot), spec) in parts.into_iter().zip(pivots).zip(boxes) {
        let place = Mat4::from_translation(Vec3::from_array(pivot) + Vec3::from_array(offset))
            * Mat4::from_rotation_y(turn);
        if spec.size[2] == 0.0 {
            builder.sheet(Layer::Solid, &texture, matrix * place, spec, THIN, WHITE);
        } else {
            builder.cuboid(Layer::Solid, &texture, matrix * place, spec, WHITE);
        }
    }
    let seam = BoxSpec::new([-1.0, -5.0, 0.0], [2.0, 10.0, 0.0], [12.0, 0.0]);
    builder.sheet(
        Layer::Solid,
        &texture,
        matrix * Mat4::from_rotation_y(FRAC_PI_2),
        seam,
        THIN,
        WHITE,
    );
}

/// The model-to-world matrix of a table's book at `pose`.
fn table_matrix(block: [i32; 3], pose: BookPose) -> Mat4 {
    let bob = BOB_BLOCKS * (pose.time * BOB_RATE).sin();
    Mat4::from_translation(Vec3::new(
        block[0] as f32 + 0.5,
        block[1] as f32 + HOVER_BLOCKS + bob,
        block[2] as f32 + 0.5,
    )) * Mat4::from_rotation_y(-pose.rot)
        * Mat4::from_rotation_z(LEAN_RADIANS)
        * Mat4::from_scale(Vec3::splat(1.0 / 16.0))
}

/// Emits the enchanting table's book at `block` in `pose`.
pub(super) fn emit_enchant_table(
    builder: &mut MeshBuilder,
    atlas: &BlockEntityAtlas,
    block: [i32; 3],
    pose: BookPose,
) {
    emit_book(builder, atlas, table_matrix(block, pose), table_parts(pose));
}

/// Emits the lectern stand at `block` facing `facing_yaw_degrees`, with its book when it holds
/// one.
pub(super) fn emit_lectern(
    builder: &mut MeshBuilder,
    atlas: &BlockEntityAtlas,
    block: [i32; 3],
    facing_yaw_degrees: f32,
    has_book: bool,
) {
    let base = model_matrix(block, [0.5, 0.0, 0.5], facing_yaw_degrees);
    let [Some(bottom), Some(sides), Some(top), Some(front)] =
        geometry::LECTERN_TEXTURES.map(|name| atlas.texture(name, [16.0; 2]))
    else {
        return;
    };
    let textures = [bottom, sides, top, front];
    for (index, (texture_index, (corners, texels))) in
        geometry::lectern_faces().into_iter().enumerate()
    {
        let texture = textures[texture_index];
        let [u0, v0, u1, v1] = texture.rect_uv(texels);
        let shade = super::mesh::tile_face_shade(index % 6);
        builder.quad_uv(
            Layer::Solid,
            corners.map(|corner| base.transform_point3(Vec3::from_array(corner)).to_array()),
            [[u0, v0], [u1, v0], [u1, v1], [u0, v1]],
            [shade, shade, shade, 1.0],
        );
    }
    if has_book {
        let board = base
            * Mat4::from_translation(Vec3::from_array(geometry::LECTERN_BOARD_OFFSET))
            * Mat4::from_translation(Vec3::from_array(geometry::LECTERN_BOARD_PIVOT))
            * Mat4::from_rotation_x(-geometry::LECTERN_SLOPE_DEGREES.to_radians())
            * Mat4::from_translation(-Vec3::from_array(geometry::LECTERN_BOARD_PIVOT));
        let book = board
            * Mat4::from_translation(Vec3::new(
                0.0,
                geometry::LECTERN_BOARD[1][1] + 0.5,
                (geometry::LECTERN_BOARD[0][2] + geometry::LECTERN_BOARD[1][2]) * 0.5,
            ))
            * Mat4::from_rotation_x(-FRAC_PI_2)
            * Mat4::from_rotation_y(-FRAC_PI_2);
        emit_book(builder, atlas, book, LECTERN_PARTS);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_near_reader_opens_the_book_toward_them_and_leaving_closes_it() {
        let mut book = EnchantingBook::new([4, 64, -3]);
        let reader = Vec3::new(4.5, 65.62, -1.0);
        for _ in 0..30 {
            book.tick(Some(reader));
        }
        let pose = book.pose(1.0);
        assert_eq!(pose.open, 1.0, "ten ticks at 0.1 open it fully");
        let facing = Vec3::new(pose.rot.cos(), 0.0, pose.rot.sin());
        assert!(
            facing.abs_diff_eq(Vec3::Z, 1.0e-3),
            "pages face the reader: {facing}"
        );
        for _ in 0..5 {
            book.tick(None);
        }
        assert!((book.pose(1.0).open - 0.5).abs() < 1.0e-5);
        for _ in 0..10 {
            book.tick(None);
        }
        assert_eq!(book.pose(1.0).open, 0.0);
    }

    #[test]
    fn pages_chase_new_targets_while_opening_at_a_capped_speed() {
        let mut book = EnchantingBook::new([0; 3]);
        let reader = Some(Vec3::new(0.5, 1.62, 2.0));
        let mut targets = Vec::new();
        for _ in 0..200 {
            let before = book.pose(1.0).flip;
            book.tick(reader);
            assert!((book.pose(1.0).flip - before).abs() <= FLIP_MAX_SPEED + 1.0e-6);
            targets.push(book.flip_target);
        }
        assert!(
            targets.windows(2).take(3).all(|pair| pair[0] != pair[1]),
            "every tick below half open picks a new target"
        );
        let resting = targets.windows(2).filter(|pair| pair[0] == pair[1]).count();
        assert!(resting > 150, "open books keep a target for about 40 ticks");
    }

    #[test]
    fn only_players_within_three_blocks_of_the_book_centre_read_it() {
        let book = EnchantingBook::new([0; 3]);
        let near = Vec3::new(1.5, 2.62, 0.5);
        let nearer = Vec3::new(0.5, 2.0, 1.5);
        let far = Vec3::new(3.6, 1.0, 0.5);
        assert_eq!(book.nearest_reader([far]), None);
        assert_eq!(book.nearest_reader([far, near, nearer]), Some(nearer));
    }

    #[test]
    fn each_loose_page_rests_then_turns_once_per_cycle() {
        let samples: Vec<[f32; 2]> = (0..100)
            .map(|step| page_turns(step as f32 / 100.0))
            .collect();
        for page in 0..2 {
            assert!(samples.iter().any(|pages| pages[page] == 0.0));
            assert!(samples.iter().any(|pages| pages[page] == 1.0));
        }
    }

    #[test]
    fn backward_page_turns_follow_the_same_cycle_as_forward_turns() {
        for step in 0..128 {
            let progress = step as f32 / 128.0;
            let forward = page_turns(progress);
            for cycle in [-16.0, -4.0, -1.0, 0.0, 1.0, 4.0] {
                assert_eq!(
                    page_turns(progress + cycle),
                    forward,
                    "page progress {progress} in cycle {cycle}"
                );
            }
        }
    }

    #[test]
    fn a_closed_book_stacks_its_covers_and_an_open_one_spreads_them() {
        let closed = table_parts(BookPose::default());
        assert_eq!(closed[0].0, PI);
        assert_eq!(closed[1].0, 0.0);
        let open = table_parts(BookPose {
            open: 1.0,
            ..BookPose::default()
        });
        assert!((open[1].0 + COVER_OPEN_RADIANS).abs() < 1.0e-6);
        assert!((open[2].1[0] - COVER_OPEN_RADIANS.sin()).abs() < 1.0e-6);
    }

    #[test]
    fn the_table_book_hovers_and_leans_its_pages_up_toward_the_reader() {
        let rot = 0.7_f32;
        let matrix = table_matrix(
            [0; 3],
            BookPose {
                rot,
                ..BookPose::default()
            },
        );
        let normal = matrix.transform_vector3(Vec3::X).normalize();
        let toward = Vec3::new(rot.cos(), 0.0, rot.sin());
        assert!(normal.y > 0.95, "mostly up: {normal}");
        assert!(
            normal.dot(toward) > 0.15,
            "leans toward the reader: {normal}"
        );
        let spine = matrix.transform_point3(Vec3::ZERO);
        assert!((spine.y - HOVER_BLOCKS).abs() < 1.0e-5);
    }

    #[test]
    fn the_lectern_book_opens_upward() {
        let normal = (Mat4::from_rotation_x(-FRAC_PI_2) * Mat4::from_rotation_y(-FRAC_PI_2))
            .transform_vector3(Vec3::X);
        assert!(normal.abs_diff_eq(Vec3::Y, 1.0e-5));
    }

    #[test]
    fn the_lectern_slope_lowers_the_reader_edge() {
        let slope = Mat4::from_rotation_x(-geometry::LECTERN_SLOPE_DEGREES.to_radians());
        let reader_edge = slope.transform_point3(Vec3::new(0.0, 0.0, -8.0));
        let far_edge = slope.transform_point3(Vec3::new(0.0, 0.0, 8.0));
        assert!(reader_edge.y < far_edge.y);
    }
    #[test]
    fn lectern_stand_uses_cropped_faces_and_the_sloped_board_dimensions() {
        let mut placements: Vec<_> = [
            "lectern_base",
            "lectern_sides",
            "lectern_top",
            "lectern_front",
        ]
        .into_iter()
        .enumerate()
        .map(|(index, name)| assets::BlockEntityPlacement {
            name: format!("textures/blocks/{name}").into(),
            x: index as u32 * 16,
            y: 0,
            width: 16,
            height: 16,
        })
        .collect();
        placements.sort_unstable_by(|left, right| left.name.cmp(&right.name));
        let encoded = assets::encode_block_entity_catalog(
            b"{}",
            64,
            16,
            &vec![255; 64 * 16 * 4],
            &placements,
        )
        .unwrap();
        let atlas = BlockEntityAtlas::from_assets(
            &assets::RuntimeBlockEntityAssets::decode(&encoded).unwrap(),
        );
        for yaw in [0.0, 90.0, 180.0, 270.0] {
            let mut builder = MeshBuilder::new(atlas.size());
            emit_lectern(&mut builder, &atlas, [0; 3], yaw, false);
            let inverse = model_matrix([0; 3], [0.5, 0.0, 0.5], yaw).inverse();
            let points: Vec<_> = builder
                .solid
                .iter()
                .map(|vertex| inverse.transform_point3(Vec3::from_array(vertex.position)))
                .collect();
            let board = &points[72..];
            assert!(((board[0] - board[1]).length() - 4.0).abs() < 1.0e-4);
            assert!(((board[0] - board[2]).length() - 13.0).abs() < 1.0e-4);
            let (min_x, max_x) = board
                .iter()
                .fold((f32::INFINITY, f32::NEG_INFINITY), |(min, max), point| {
                    (min.min(point.x), max.max(point.x))
                });
            assert!((max_x - min_x - 15.8).abs() < 1.0e-4);
            let front = &builder.solid[60..66];
            for (vertex, point) in front.iter().zip(&points[60..66]) {
                let expected = [
                    if (point.y - geometry::LECTERN_POST[1][1]).abs() < 1.0e-4 {
                        56.0
                    } else {
                        48.0
                    },
                    if point.x > 0.0 { 0.0 } else { 13.0 },
                ];
                let uv = [
                    vertex.uv[0] * atlas.size()[0] as f32,
                    vertex.uv[1] * atlas.size()[1] as f32,
                ];
                assert!(
                    (uv[0] - expected[0]).abs() < 1.0e-4 && (uv[1] - expected[1]).abs() < 1.0e-4
                );
            }
            let top = &builder.solid[90..96];
            let min_v = top
                .iter()
                .map(|vertex| vertex.uv[1] * atlas.size()[1] as f32)
                .reduce(f32::min)
                .unwrap();
            let max_v = top
                .iter()
                .map(|vertex| vertex.uv[1] * atlas.size()[1] as f32)
                .reduce(f32::max)
                .unwrap();
            assert!((min_v - 1.0).abs() < 1.0e-4 && (max_v - 14.0).abs() < 1.0e-4);
        }
    }
}
