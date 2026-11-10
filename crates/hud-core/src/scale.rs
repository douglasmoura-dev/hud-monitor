//! Window scale: the factor that multiplies every length of the HUD and of the mini bar.

use crate::position::Size;

pub const MIN_SCALE: f32 = 0.6;
pub const MAX_SCALE: f32 = 2.0;
pub const DEFAULT_SCALE: f32 = 1.0;

/// Presses `scale` into the allowed range; a value that is not a number becomes the default.
pub fn clamp_scale(scale: f32) -> f32 {
    if scale.is_nan() {
        DEFAULT_SCALE
    } else {
        scale.clamp(MIN_SCALE, MAX_SCALE)
    }
}

/// Window size at `scale`, rounded to whole pixels.
pub fn scaled_size(base: Size, scale: f32) -> Size {
    let scale = clamp_scale(scale);
    let scaled = |length: i32| (length as f32 * scale).round() as i32;
    Size {
        width: scaled(base.width),
        height: scaled(base.height),
    }
}

/// New scale after the resize handle moved by (`dx`, `dy`) logical pixels while the window
/// measured `window` (width, height). The drag is projected on the window diagonal so the
/// aspect ratio never changes.
pub fn scale_after_drag(scale: f32, window: (f32, f32), dx: f32, dy: f32) -> f32 {
    let scale = clamp_scale(scale);
    let (w, h) = window;
    let diagonal = w * w + h * h;
    if !(w > 0.0 && h > 0.0 && diagonal.is_finite() && dx.is_finite() && dy.is_finite()) {
        return scale;
    }
    let grown = (w + dx) * w + (h + dy) * h;
    // whole percents keep the saved config tidy
    clamp_scale((scale * grown / diagonal * 100.0).round() / 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn clamp_keeps_the_range_and_presses_outliers() {
        assert_eq!(clamp_scale(1.0), 1.0);
        assert_eq!(clamp_scale(0.6), 0.6);
        assert_eq!(clamp_scale(2.0), 2.0);
        assert_eq!(clamp_scale(5.0), 2.0);
        assert_eq!(clamp_scale(0.1), 0.6);
        assert_eq!(clamp_scale(-3.0), 0.6);
        assert_eq!(clamp_scale(f32::INFINITY), 2.0);
        assert_eq!(clamp_scale(f32::NAN), 1.0);
    }

    #[test]
    fn scaled_size_multiplies_and_rounds() {
        let base = Size {
            width: 320,
            height: 500,
        };
        assert_eq!(scaled_size(base, 1.0), base);
        assert_eq!(
            scaled_size(base, 1.5),
            Size {
                width: 480,
                height: 750
            }
        );
        assert_eq!(
            scaled_size(
                Size {
                    width: 101,
                    height: 33
                },
                0.6
            ),
            Size {
                width: 61,
                height: 20
            }
        );
    }

    #[test]
    fn scaled_size_clamps_the_scale_first() {
        let base = Size {
            width: 100,
            height: 100,
        };
        assert_eq!(scaled_size(base, 9.0).width, 200);
        assert_eq!(scaled_size(base, 0.0).width, 60);
    }

    #[test]
    fn dragging_the_corner_fifty_percent_outwards_gives_one_and_a_half() {
        let s = scale_after_drag(1.0, (320.0, 500.0), 160.0, 250.0);
        assert!(near(s, 1.5), "{s}");
    }

    #[test]
    fn dragging_inwards_shrinks() {
        let s = scale_after_drag(1.0, (320.0, 500.0), -64.0, -100.0);
        assert!(near(s, 0.8), "{s}");
    }

    #[test]
    fn drag_is_relative_to_the_current_scale() {
        // already 1.5x (480x750): another +20% of the current size makes 1.8
        let s = scale_after_drag(1.5, (480.0, 750.0), 96.0, 150.0);
        assert!(near(s, 1.8), "{s}");
    }

    #[test]
    fn an_off_diagonal_drag_is_projected_so_the_proportion_holds() {
        let s = scale_after_drag(1.0, (300.0, 400.0), 100.0, 0.0);
        assert!(s > 1.0 && s < 1.0 + 100.0 / 300.0, "{s}");
        // a drag perpendicular to the diagonal changes nothing
        let s = scale_after_drag(1.0, (300.0, 400.0), 40.0, -30.0);
        assert!(near(s, 1.0), "{s}");
    }

    #[test]
    fn drag_never_leaves_the_range() {
        assert_eq!(scale_after_drag(1.9, (608.0, 950.0), 900.0, 900.0), 2.0);
        assert_eq!(scale_after_drag(0.7, (224.0, 350.0), -900.0, -900.0), 0.6);
    }

    #[test]
    fn the_scale_after_a_drag_is_a_whole_percent() {
        let s = scale_after_drag(1.0, (320.0, 500.0), 37.0, 11.0);
        assert!(near(s * 100.0, (s * 100.0).round()), "{s}");
    }

    #[test]
    fn a_window_without_size_keeps_the_scale() {
        assert_eq!(scale_after_drag(1.2, (0.0, 0.0), 10.0, 10.0), 1.2);
        assert_eq!(scale_after_drag(1.2, (f32::NAN, 5.0), 10.0, 10.0), 1.2);
        assert!(near(
            scale_after_drag(f32::NAN, (10.0, 10.0), 1.0, 1.0),
            1.1
        ));
    }
}
