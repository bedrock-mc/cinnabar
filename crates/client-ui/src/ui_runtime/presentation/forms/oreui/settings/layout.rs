//! Settings removes grid gutters below the desktop breakpoint.

pub(super) struct Columns {
    pub(super) navigation: [f32; 2],
    pub(super) content: [f32; 2],
    pub(super) content_width: f32,
}

impl Columns {
    pub(super) fn new(rem: f32, width: f32) -> Self {
        let desktop = width >= 128.0 * rem;
        let row = if desktop {
            (width - 4.8 * rem).min(128.0 * rem)
        } else {
            width
        };
        let left = (width - row) * 0.5;
        let pad = if desktop { 0.8 * rem } else { 0.0 };
        let navigation_fraction = if width < 70.0 * rem {
            3.0 / 8.0
        } else {
            4.0 / 12.0
        };
        let boundary = left + row * navigation_fraction;
        Self {
            navigation: [left + pad, boundary - pad],
            content: [boundary + pad, left + row - pad],
            content_width: row * (1.0 - navigation_fraction),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Columns;

    #[test]
    fn narrow_and_tablet_settings_use_the_full_width() {
        for width in [600.0, 900.0] {
            let columns = Columns::new(10.0, width);
            assert_eq!(columns.navigation[0], 0.0);
            assert_eq!(columns.navigation[1], columns.content[0]);
            assert_eq!(columns.content[1], width);
            assert!((columns.content_width - (width - columns.content[0])).abs() < 0.001);
        }
    }

    #[test]
    fn desktop_caps_the_row_after_outer_padding() {
        let near_breakpoint = Columns::new(10.0, 1300.0);
        assert_eq!(near_breakpoint.navigation[0], 32.0);
        assert_eq!(near_breakpoint.content[1], 1268.0);
        let wide = Columns::new(10.0, 2000.0);
        assert_eq!(wide.navigation[0], 368.0);
        assert_eq!(wide.content[1], 1632.0);
        assert!((wide.content_width - 1280.0 * 2.0 / 3.0).abs() < 0.001);
    }
}
