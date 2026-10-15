//! The Discord mark for the crash window's community button, rasterized with anti-aliasing at the
//! exact pixel size from the same SVG path data as the website's Discord link.

use std::f32::consts::{FRAC_PI_2, TAU};

use tiny_skia::{FillRule, Paint, Path, PathBuilder, Pixmap, Transform};

use bootstrap_ui::canvas::Image;

/// The mark in a 24-unit view box, copied from the Discord link in `website/index.html.in`; a test
/// keeps the two identical.
const MARK: &str = concat!(
    "M20.317 4.3698a19.7913 19.7913 0 00-4.8851-1.5152.0741.0741 0 00-.0785.0371",
    "c-.211.3753-.4447.8648-.6083 1.2495-1.8447-.2762-3.68-.2762-5.4868 0-.1636-.3933-.4058-.8742-.6177-1.2495",
    "a.077.077 0 00-.0785-.037 19.7363 19.7363 0 00-4.8852 1.515.0699.0699 0 00-.0321.0277",
    "C.5334 9.0458-.319 13.5799.0992 18.0578a.0824.0824 0 00.0312.0561",
    "c2.0528 1.5076 4.0413 2.4228 5.9929 3.0294a.0777.0777 0 00.0842-.0276",
    "c.4616-.6304.8731-1.2952 1.226-1.9942a.076.076 0 00-.0416-.1057",
    "c-.6528-.2476-1.2743-.5495-1.8722-.8923a.077.077 0 01-.0076-.1277",
    "c.1258-.0943.2517-.1923.3718-.2914a.0743.0743 0 01.0776-.0105",
    "c3.9278 1.7933 8.18 1.7933 12.0614 0a.0739.0739 0 01.0785.0095",
    "c.1202.099.246.1981.3728.2924",
    "a.077.077 0 01-.0066.1276 12.2986 12.2986 0 01-1.873.8914.0766.0766 0 00-.0407.1067",
    "c.3604.698.7719 1.3628 1.225 1.9932a.076.076 0 00.0842.0286",
    "c1.961-.6067 3.9495-1.5219 6.0023-3.0294a.077.077 0 00.0313-.0552",
    "c.5004-5.177-.8382-9.6739-3.5485-13.6604a.061.061 0 00-.0312-.0286zM8.02 15.3312",
    "c-1.1825 0-2.1569-1.0857-2.1569-2.419 0-1.3332.9555-2.4189 2.157-2.4189 1.2108 0 2.1757 1.0952 2.1568 2.419 0 1.3332-.9555 2.4189-2.1569 2.4189",
    "zm7.9748 0",
    "c-1.1825 0-2.1569-1.0857-2.1569-2.419 0-1.3332.9554-2.4189 2.1569-2.4189 1.2108 0 2.1757 1.0952 2.1568 2.419 0 1.3332-.946 2.4189-2.1568 2.4189",
    "Z",
);
const VIEW_BOX: f32 = 24.0;

/// The white mark, parsed once and re-rasterized only when its pixel size changes.
pub(super) struct DiscordMark {
    path: Option<Path>,
    raster: Option<Image>,
}

impl DiscordMark {
    pub(super) fn new() -> Self {
        Self {
            path: parse(MARK),
            raster: None,
        }
    }

    /// The mark `size` physical pixels square, in straight-alpha white.
    pub(super) fn image(&mut self, size: u32) -> Option<&Image> {
        if self.raster.as_ref().is_none_or(|image| image.width != size) {
            self.raster = self.path.as_ref().and_then(|path| rasterize(path, size));
        }
        self.raster.as_ref()
    }
}

/// Fills `path` white with SVG's default nonzero rule, scaled from the view box to `size`.
fn rasterize(path: &Path, size: u32) -> Option<Image> {
    let mut pixmap = Pixmap::new(size, size)?;
    let mut paint = Paint::default();
    paint.set_color_rgba8(255, 255, 255, 255);
    paint.anti_alias = true;
    let scale = size as f32 / VIEW_BOX;
    pixmap.fill_path(
        path,
        &paint,
        FillRule::Winding,
        Transform::from_scale(scale, scale),
        None,
    );
    // Premultiplied white is straight white at the pixel's coverage.
    let rgba = pixmap
        .data()
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|pixel| [255, 255, 255, pixel[3]])
        .collect();
    Some(Image {
        width: size,
        height: size,
        rgba,
    })
}

/// SVG path data tokens: command letters, numbers in compact form (`.07.07`, `-1-2`) and
/// single-digit arc flags.
struct Lexer<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Lexer<'_> {
    fn skip_separators(&mut self) {
        while self
            .bytes
            .get(self.at)
            .is_some_and(|byte| byte.is_ascii_whitespace() || *byte == b',')
        {
            self.at += 1;
        }
    }

    /// The next command letter, if one comes before the next number.
    fn command(&mut self) -> Option<u8> {
        self.skip_separators();
        let byte = *self.bytes.get(self.at)?;
        byte.is_ascii_alphabetic().then(|| {
            self.at += 1;
            byte
        })
    }

    /// True when a number follows, continuing the current command implicitly.
    fn number_follows(&mut self) -> bool {
        self.skip_separators();
        self.bytes
            .get(self.at)
            .is_some_and(|byte| byte.is_ascii_digit() || matches!(byte, b'.' | b'-' | b'+'))
    }

    fn number(&mut self) -> Option<f32> {
        self.skip_separators();
        let start = self.at;
        let digits = |lexer: &mut Self| {
            while lexer.bytes.get(lexer.at).is_some_and(u8::is_ascii_digit) {
                lexer.at += 1;
            }
        };
        if matches!(self.bytes.get(self.at), Some(b'-' | b'+')) {
            self.at += 1;
        }
        digits(self);
        if self.bytes.get(self.at) == Some(&b'.') {
            self.at += 1;
            digits(self);
        }
        if matches!(self.bytes.get(self.at), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.bytes.get(self.at), Some(b'-' | b'+')) {
                self.at += 1;
            }
            digits(self);
        }
        std::str::from_utf8(&self.bytes[start..self.at])
            .ok()?
            .parse()
            .ok()
    }

    /// An arc flag: exactly one `0` or `1`, which may run into the next number.
    fn flag(&mut self) -> Option<bool> {
        self.skip_separators();
        let flag = match self.bytes.get(self.at)? {
            b'0' => false,
            b'1' => true,
            _ => return None,
        };
        self.at += 1;
        Some(flag)
    }

    fn point(&mut self, origin: (f32, f32)) -> Option<(f32, f32)> {
        Some((origin.0 + self.number()?, origin.1 + self.number()?))
    }
}

/// Builds a path from SVG path data using M, L, H, V, C, A and Z (absolute or relative) with
/// implicit command repeats; anything else is rejected.
fn parse(data: &str) -> Option<Path> {
    let mut lexer = Lexer {
        bytes: data.as_bytes(),
        at: 0,
    };
    let mut builder = PathBuilder::new();
    let (mut current, mut start) = ((0.0, 0.0), (0.0, 0.0));
    let mut command = None;
    loop {
        if let Some(next) = lexer.command() {
            command = Some(next);
        } else if !lexer.number_follows() {
            break;
        }
        let letter = command?;
        let origin = if letter.is_ascii_lowercase() {
            current
        } else {
            (0.0, 0.0)
        };
        match letter.to_ascii_uppercase() {
            b'M' => {
                current = lexer.point(origin)?;
                start = current;
                builder.move_to(current.0, current.1);
                // Further pairs after a move are lines.
                command = Some(if letter == b'm' { b'l' } else { b'L' });
            }
            b'L' => {
                current = lexer.point(origin)?;
                builder.line_to(current.0, current.1);
            }
            b'H' => {
                current.0 = origin.0 + lexer.number()?;
                builder.line_to(current.0, current.1);
            }
            b'V' => {
                current.1 = origin.1 + lexer.number()?;
                builder.line_to(current.0, current.1);
            }
            b'C' => {
                let (first, second, end) = (
                    lexer.point(origin)?,
                    lexer.point(origin)?,
                    lexer.point(origin)?,
                );
                builder.cubic_to(first.0, first.1, second.0, second.1, end.0, end.1);
                current = end;
            }
            b'A' => {
                let radii = (lexer.number()?, lexer.number()?);
                let rotation = lexer.number()?;
                let (large, sweep) = (lexer.flag()?, lexer.flag()?);
                let end = lexer.point(origin)?;
                arc_to(&mut builder, current, radii, rotation, large, sweep, end);
                current = end;
            }
            b'Z' => {
                builder.close();
                current = start;
                command = None;
            }
            _ => return None,
        }
    }
    builder.finish()
}

/// Appends an SVG elliptical arc as cubic Béziers of at most a quarter turn each, converting the
/// endpoint form to the centre form (SVG 1.1 appendix F.6.5).
fn arc_to(
    builder: &mut PathBuilder,
    from: (f32, f32),
    radii: (f32, f32),
    rotation: f32,
    large: bool,
    sweep: bool,
    to: (f32, f32),
) {
    if from == to {
        return;
    }
    let (mut rx, mut ry) = (radii.0.abs(), radii.1.abs());
    if rx == 0.0 || ry == 0.0 {
        builder.line_to(to.0, to.1);
        return;
    }
    let (sin, cos) = rotation.to_radians().sin_cos();
    let (half_x, half_y) = ((from.0 - to.0) * 0.5, (from.1 - to.1) * 0.5);
    let x1 = cos * half_x + sin * half_y;
    let y1 = -sin * half_x + cos * half_y;
    let reach = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
    if reach > 1.0 {
        rx *= reach.sqrt();
        ry *= reach.sqrt();
    }
    let numerator = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
    let denominator = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let mut scale = (numerator / denominator).max(0.0).sqrt();
    if large == sweep {
        scale = -scale;
    }
    let (cx1, cy1) = (scale * rx * y1 / ry, -scale * ry * x1 / rx);
    let centre = (
        cos * cx1 - sin * cy1 + (from.0 + to.0) * 0.5,
        sin * cx1 + cos * cy1 + (from.1 + to.1) * 0.5,
    );
    let angle = |ux: f32, uy: f32, vx: f32, vy: f32| (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
    let start = angle(1.0, 0.0, (x1 - cx1) / rx, (y1 - cy1) / ry);
    let mut sweep_angle = angle(
        (x1 - cx1) / rx,
        (y1 - cy1) / ry,
        (-x1 - cx1) / rx,
        (-y1 - cy1) / ry,
    );
    if !sweep && sweep_angle > 0.0 {
        sweep_angle -= TAU;
    } else if sweep && sweep_angle < 0.0 {
        sweep_angle += TAU;
    }
    let segments = (sweep_angle.abs() / FRAC_PI_2).ceil().max(1.0);
    let step = sweep_angle / segments;
    let handle = 4.0 / 3.0 * (step / 4.0).tan();
    let map = |x: f32, y: f32| {
        (
            cos * rx * x - sin * ry * y + centre.0,
            sin * rx * x + cos * ry * y + centre.1,
        )
    };
    let mut theta = start;
    for index in 0..segments as usize {
        let (sin1, cos1) = theta.sin_cos();
        let (sin2, cos2) = (theta + step).sin_cos();
        let first = map(cos1 - handle * sin1, sin1 + handle * cos1);
        let second = map(cos2 + handle * sin2, sin2 - handle * cos2);
        // The last segment ends exactly on the requested point.
        let end = if index + 1 == segments as usize {
            to
        } else {
            map(cos2, sin2)
        };
        builder.cubic_to(first.0, first.1, second.0, second.1, end.0, end.1);
        theta += step;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mark_matches_the_website_discord_link() {
        let website = include_str!("../../../../website/index.html.in");
        assert!(website.contains(&format!("<path d=\"{MARK}\"")));
    }

    #[test]
    fn compact_numbers_and_flags_split_like_svg() {
        let mut lexer = Lexer {
            bytes: b".0741.0741 0 00-4.8851-1.5",
            at: 0,
        };
        assert_eq!(lexer.number(), Some(0.0741));
        assert_eq!(lexer.number(), Some(0.0741));
        assert_eq!(lexer.number(), Some(0.0));
        assert_eq!(lexer.flag(), Some(false));
        assert_eq!(lexer.flag(), Some(false));
        assert_eq!(lexer.number(), Some(-4.8851));
        assert_eq!(lexer.number(), Some(-1.5));
    }

    #[test]
    fn arcs_end_on_their_endpoint() {
        let mut builder = PathBuilder::new();
        builder.move_to(0.0, 0.0);
        arc_to(
            &mut builder,
            (0.0, 0.0),
            (5.0, 5.0),
            0.0,
            false,
            true,
            (10.0, 0.0),
        );
        let path = builder.finish().unwrap();
        let bounds = path.bounds();
        assert!((bounds.right() - 10.0).abs() < 1e-4);
        assert!((bounds.top() + 5.0).abs() < 0.01 || (bounds.bottom() - 5.0).abs() < 0.01);
    }

    #[test]
    fn mark_rasterizes_with_soft_edges_and_eye_holes() {
        let mut mark = DiscordMark::new();
        let image = mark.image(48).expect("the mark parses and rasterizes");
        let alpha = |x: u32, y: u32| image.rgba[((y * 48 + x) * 4 + 3) as usize];
        // Solid face below the eyes, open corners, holes for the eyes and some partial coverage.
        assert_eq!(alpha(24, 34), 255);
        assert_eq!(alpha(0, 0), 0);
        assert_eq!(alpha(17, 26), 0);
        assert!(
            image
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| (1..255).contains(&pixel[3]))
        );
    }
}
