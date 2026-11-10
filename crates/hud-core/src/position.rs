//! Window placement math, independent of any windowing system.

/// Gap between the HUD and the screen edges on first run.
pub const MARGIN: i32 = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub width: i32,
    pub height: i32,
}

/// Top-right corner with a 12 px margin: x = screen_width - window_width - 12, y = 12.
pub fn first_run_position(screen: Size, window: Size) -> Point {
    Point {
        x: screen.width - window.width - MARGIN,
        y: MARGIN,
    }
}

/// Moves `pos` so the whole window lies inside the screen (top-left pinned if it cannot fit).
pub fn clamp_to_screen(pos: Point, window: Size, screen: Size) -> Point {
    Point {
        x: pos.x.clamp(0, (screen.width - window.width).max(0)),
        y: pos.y.clamp(0, (screen.height - window.height).max(0)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Size = Size {
        width: 1920,
        height: 1080,
    };
    const WINDOW: Size = Size {
        width: 320,
        height: 200,
    };

    #[test]
    fn first_run_is_top_right_with_margin() {
        assert_eq!(
            first_run_position(SCREEN, WINDOW),
            Point {
                x: 1920 - 320 - 12,
                y: 12
            }
        );
    }

    #[test]
    fn saved_position_inside_screen_is_kept() {
        let p = Point { x: 400, y: 300 };
        assert_eq!(clamp_to_screen(p, WINDOW, SCREEN), p);
    }

    #[test]
    fn far_off_screen_position_is_pulled_inside() {
        let p = clamp_to_screen(Point { x: 5000, y: 5000 }, WINDOW, SCREEN);
        assert_eq!(p, Point { x: 1600, y: 880 });
    }

    #[test]
    fn negative_position_is_pulled_to_origin() {
        let p = clamp_to_screen(Point { x: -900, y: -50 }, WINDOW, SCREEN);
        assert_eq!(p, Point { x: 0, y: 0 });
    }

    #[test]
    fn window_larger_than_screen_pins_top_left() {
        let tiny = Size {
            width: 100,
            height: 100,
        };
        assert_eq!(
            clamp_to_screen(Point { x: 50, y: 50 }, WINDOW, tiny),
            Point { x: 0, y: 0 }
        );
    }

    #[test]
    fn a_scaled_window_near_the_edge_is_pulled_in_by_its_scaled_size() {
        use crate::scale::scaled_size;
        let big = scaled_size(WINDOW, 2.0);
        assert_eq!(
            clamp_to_screen(Point { x: 1500, y: 800 }, big, SCREEN),
            Point { x: 1280, y: 680 }
        );
        // the unscaled size would have left it hanging off the screen
        assert_eq!(
            clamp_to_screen(Point { x: 1500, y: 800 }, WINDOW, SCREEN),
            Point { x: 1500, y: 800 }
        );
    }

    #[test]
    fn a_hud_scaled_beyond_the_screen_height_pins_to_the_top() {
        use crate::config::HUD_SIZE;
        use crate::scale::scaled_size;
        let p = clamp_to_screen(Point { x: 100, y: 300 }, scaled_size(HUD_SIZE, 2.0), SCREEN);
        assert_eq!((p.x, p.y), (100, 0));
    }
}
