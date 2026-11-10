//! Slint presentation: the generated window types, the mapping of core view models onto them and
//! the window host. Decisions live in `hud-core`; this module only draws.

mod mapping;
mod theme;
mod windows;

pub use mapping::{clock_text, show_dashboard};
pub use theme::slint_color;
pub use windows::{apply_config, connect, Controller, Looks, SlintWindows};

slint::include_modules!();

pub use theme::{paint_hud, paint_minibar, paint_scanner};

#[cfg(test)]
mod tests {
    const HUD: &str = include_str!("../../ui/hud.slint");
    const THEME: &str = include_str!("../../ui/theme.slint");
    const MINIBAR: &str = include_str!("../../ui/minibar.slint");
    const SCANNER: &str = include_str!("../../ui/scanner.slint");

    fn families(src: &str) -> Vec<&str> {
        src.lines()
            .filter_map(|l| l.trim().strip_prefix("font-family:"))
            .map(|v| v.trim().trim_end_matches(';'))
            .collect()
    }

    /// Every bundled font file with the OFL license that must sit next to it.
    const FONTS: [(&str, &str); 6] = [
        ("Rajdhani-Medium.ttf", "OFL-rajdhani.txt"),
        ("ShareTechMono-Regular.ttf", "OFL-sharetechmono.txt"),
        ("Orbitron-Variable.ttf", "OFL-orbitron.txt"),
        ("ChakraPetch-Medium.ttf", "OFL-chakrapetch.txt"),
        ("Inter-Light.ttf", "OFL-inter.txt"),
        ("VT323-Regular.ttf", "OFL-vt323.txt"),
    ];

    #[test]
    fn theme_defaults_are_the_gamer_hud_fonts() {
        assert!(THEME.contains(r#"label-font: "Rajdhani""#));
        assert!(THEME.contains(r#"number-font: "Share Tech Mono""#));
        assert!(THEME.contains(r#"title-font: "Rajdhani""#));
    }

    #[test]
    fn the_new_fonts_are_embedded_and_each_has_its_ofl_file_next_to_it() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/fonts");
        for (font, license) in FONTS {
            assert!(
                THEME.contains(&format!("fonts/{font}")),
                "{font} not imported"
            );
            assert!(dir.join(font).is_file(), "{font} missing");
            let text = std::fs::read_to_string(dir.join(license))
                .unwrap_or_else(|e| panic!("{license}: {e}"));
            assert!(
                text.contains("SIL OPEN FONT LICENSE Version 1.1"),
                "{license}"
            );
        }
        assert!(THEME.contains("fonts/Rajdhani-SemiBold.ttf"));
        assert!(THEME.contains("fonts/ChakraPetch-SemiBold.ttf"));
        assert!(THEME.contains("fonts/Inter-Medium.ttf"));
    }

    #[test]
    fn every_font_family_a_theme_names_is_bundled_or_the_system_one() {
        use hud_core::theme::ThemeId;
        for theme in ThemeId::ALL {
            let s = theme.style();
            for family in [s.title_font, s.label_font, s.number_font] {
                let bundled = matches!(
                    family,
                    "Rajdhani"
                        | "Share Tech Mono"
                        | "Orbitron"
                        | "Chakra Petch"
                        | "Inter"
                        | "VT323"
                );
                assert!(bundled || family == "Segoe UI", "{}: {family}", theme.id());
            }
        }
    }

    #[test]
    fn every_text_in_the_hud_uses_a_theme_font() {
        let used = families(HUD);
        assert!(!used.is_empty());
        assert!(used.iter().all(|f| matches!(
            *f,
            "Theme.label-font" | "Theme.number-font" | "Theme.title-font"
        )));
        assert!(used.contains(&"Theme.number-font"));
    }

    #[test]
    fn the_hud_picks_bar_and_header_components_from_the_theme_style() {
        for style in ["thin", "rounded", "segmented", "ascii"] {
            assert!(
                HUD.contains(&format!("Theme.bar-style == \"{style}\"")),
                "{style}"
            );
        }
        assert!(HUD.contains("Theme.ascii("));
        assert!(HUD.contains("Theme.lit("));
        assert!(HUD.contains("CutBox {") && HUD.contains("Theme.scaled-cut"));
        assert!(HUD.contains("Theme.scan-tile"));
        assert!(HUD.contains("Theme.has-side-rule"));
        assert!(HUD.contains("Theme.cards") && HUD.contains("Theme.airy"));
        assert!(HUD.contains("Theme.lb(\"CPU\")"));
    }

    #[test]
    fn presence_rows_have_their_own_geometry_and_left_aligned_text() {
        let start = HUD
            .find("component PresenceRow")
            .expect("PresenceRow component");
        let row = &HUD[start
            ..HUD[start..]
                .find("component GpuCell")
                .map_or(HUD.len(), |n| start + n)];
        assert!(row.contains("x: 0px;") && row.contains("x: 100px * Theme.scale;"));
        assert_eq!(row.matches("horizontal-alignment: left;").count(), 2);
        assert!(row.contains("height: parent.height;"));
        assert!(HUD.contains("for row[i] in root.presence : PresenceRow {"));
        assert!(HUD.contains("y: root.others-y + 18px * Theme.scale + 20px * Theme.scale * i;"));
        assert!(
            HUD.contains("height: 20px * Theme.scale;")
                && HUD.contains("width: 296px * Theme.scale;")
        );
    }

    #[test]
    fn the_terminal_cursor_blinks_on_a_timer_not_a_frame_animation() {
        assert!(HUD.contains("Timer {"));
        assert!(!HUD.contains("animate"));
    }

    #[test]
    fn all_three_windows_draw_from_theme_properties_not_literal_colors() {
        for src in [HUD, MINIBAR, SCANNER] {
            assert!(src.contains("Theme.background"));
            assert!(!src.contains("rgba(10, 14, 20"));
            assert!(!src.contains("#0A0E14"));
        }
        assert!(THEME.contains("in-out property <color> warn"));
        assert!(THEME.contains("in-out property <color> crit"));
        assert!(!THEME.contains("    out property"));
        assert!(
            !THEME.contains("out property <color> teal")
                || THEME.contains("in-out property <color> teal")
        );
    }

    #[test]
    fn unavailable_values_report_their_reason_on_hover() {
        assert!(HUD.contains("root.field.tip != \"\""));
        assert!(HUD.contains("root.tip(self.has-hover ? root.field.tip"));
    }

    #[test]
    fn bar_colors_come_from_the_view_not_the_slint_file() {
        assert!(HUD.contains("bar.hot ? Theme.crit : Theme.green"));
    }

    #[test]
    fn slint_window_size_matches_hud_size() {
        use hud_core::config::{HUD_BASE_HEIGHT, HUD_SIZE};
        assert!(HUD.contains(&format!("width: {}px * Theme.scale;", HUD_SIZE.width)));
        // HUD_SIZE is the tallest layout (every provider listed); the window follows what is listed.
        assert!(HUD.contains("height: root.others-y + root.others-h + 22px * Theme.scale;"));
        assert!(HUD.contains(&format!("{}px * Theme.scale;", HUD_BASE_HEIGHT - 22)));
        assert_eq!(
            HUD_SIZE.height,
            HUD_BASE_HEIGHT + 66 + 44 + 44 + 18 + 20 * 3
        );
    }

    #[test]
    fn minibar_is_borderless_topmost_and_uses_theme_fonts() {
        assert!(MINIBAR.contains("no-frame: true;"));
        assert!(MINIBAR.contains("always-on-top: true;"));
        assert!(families(MINIBAR)
            .iter()
            .all(|f| matches!(*f, "Theme.label-font" | "Theme.number-font")));
    }

    #[test]
    fn minibar_lays_out_by_orientation_and_both_windows_expose_the_toggle() {
        assert!(MINIBAR.contains(
            "root.vertical ? 6px * Theme.scale : 30px * Theme.scale + i * 68px * Theme.scale"
        ));
        assert!(MINIBAR.contains(
            "root.vertical ? 30px * Theme.scale + i * 40px * Theme.scale : 6px * Theme.scale"
        ));
        assert!(MINIBAR.contains("root.expand()"));
        assert!(HUD.contains("root.minimize()"));
    }

    #[test]
    fn scanner_screen_uses_theme_fonts_and_asks_before_moving() {
        assert!(!families(SCANNER).is_empty());
        assert!(families(SCANNER).iter().all(|f| matches!(
            *f,
            "Theme.label-font" | "Theme.number-font" | "Theme.title-font"
        )));
        assert!(SCANNER.contains("Mover para a Lixeira"));
        assert!(SCANNER.contains("root.confirm-remove()"));
        assert!(!SCANNER.contains("Somente leitura"));
        assert!(SCANNER.contains("row.review"));
        assert!(HUD.contains("Scanner"));
    }

    #[test]
    fn hud_pin_icon_toggles_click_through_and_shows_its_state() {
        assert!(HUD.contains("root.pin()"));
        assert!(HUD.contains("root.pinned ? Theme.title-text"));
        assert!(HUD.contains("root.pin-tip"));
    }

    /// Every pixel literal of the scaled windows except the 0/1 px hairlines and the 12 px handle.
    fn unscaled_literals(src: &str) -> Vec<String> {
        let mut found = Vec::new();
        for line in src.lines() {
            let bytes = line.as_bytes();
            for (at, _) in line.match_indices("px") {
                let start = line[..at]
                    .rfind(|c: char| !(c.is_ascii_digit() || c == '.'))
                    .map_or(0, |i| i + 1);
                let number = &line[start..at];
                let follows_scale = line[at + 2..].trim_start().starts_with("* Theme.scale");
                let word_before = start > 0 && bytes[start - 1].is_ascii_alphabetic();
                if number.is_empty() || word_before || follows_scale {
                    continue;
                }
                if !matches!(number, "0" | "1" | "12") {
                    found.push(format!("{number}px in `{}`", line.trim()));
                }
            }
        }
        found
    }

    #[test]
    fn every_length_of_the_hud_and_the_mini_bar_is_multiplied_by_the_scale() {
        assert!(THEME.contains("in-out property <float> scale: 1;"));
        for (name, src) in [("hud", HUD), ("minibar", MINIBAR)] {
            let left = unscaled_literals(src);
            assert!(left.is_empty(), "{name}: {left:?}");
            assert!(!src.contains("Theme.radius") && !src.contains("Theme.corner-cut"));
            assert!(!src.contains("Theme.tracking"));
        }
        assert!(THEME.contains("scaled-radius: radius * scale"));
        assert!(THEME.contains("scaled-cut: corner-cut * scale"));
        assert!(THEME.contains("scaled-tracking: tracking * scale"));
    }

    #[test]
    fn paths_keep_their_shape_when_the_window_scales() {
        for src in [HUD, MINIBAR] {
            let paths = src.matches("Path {").count();
            assert_eq!(paths, src.matches("viewbox-width:").count());
        }
    }

    #[test]
    fn both_windows_have_a_twelve_pixel_resize_handle_in_the_bottom_right_corner() {
        for src in [HUD, MINIBAR] {
            assert!(src.contains("callback resize(length, length, length, length);"));
            assert!(src.contains("callback resize-done();"));
            assert!(src.contains("x: parent.width - 12px;"));
            assert!(src.contains("y: parent.height - 12px;"));
            assert!(src.contains("mouse-cursor: MouseCursor.nwse-resize;"));
            assert!(src.contains("root.resize(root.width, root.height,"));
            assert!(src.contains("root.resize-done()"));
        }
    }

    #[test]
    fn the_handle_is_drawn_last_so_nothing_covers_it() {
        for src in [HUD, MINIBAR] {
            let handle = src.rfind("y: parent.height - 12px;").expect("handle");
            let last_image = src.rfind("Theme.scan-tile").expect("scanlines");
            assert!(handle > last_image);
        }
    }

    #[test]
    fn no_per_frame_work_comes_with_the_scale() {
        assert!(!HUD.contains("animate") && !MINIBAR.contains("animate"));
        assert_eq!(HUD.matches("Timer {").count(), 1);
        assert_eq!(MINIBAR.matches("Timer {").count(), 0);
    }
}
