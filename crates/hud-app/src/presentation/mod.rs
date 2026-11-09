//! Slint presentation: the generated window types, the mapping of core view models onto them and
//! the window host. Decisions live in `hud-core`; this module only draws.

mod mapping;
mod windows;

pub use mapping::{clock_text, show_dashboard};
pub use windows::{apply_config, connect, Controller, Looks, SlintWindows};

slint::include_modules!();

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

    #[test]
    fn theme_maps_labels_to_rajdhani_and_numbers_to_share_tech_mono() {
        assert!(THEME.contains(r#"label-font: "Rajdhani""#));
        assert!(THEME.contains(r#"number-font: "Share Tech Mono""#));
        assert!(THEME.contains("fonts/Rajdhani-Medium.ttf"));
        assert!(THEME.contains("fonts/Rajdhani-SemiBold.ttf"));
        assert!(THEME.contains("fonts/ShareTechMono-Regular.ttf"));
    }

    #[test]
    fn every_text_in_the_hud_uses_one_of_the_two_fonts() {
        let used = families(HUD);
        assert!(!used.is_empty());
        assert!(used
            .iter()
            .all(|f| matches!(*f, "Theme.label-font" | "Theme.number-font")));
        assert!(used.contains(&"Theme.number-font"));
    }

    #[test]
    fn unavailable_values_report_their_reason_on_hover() {
        assert!(HUD.contains("root.field.tip != \"\""));
        assert!(HUD.contains("root.tip(self.has-hover ? root.field.tip"));
    }

    #[test]
    fn bar_colors_come_from_the_view_not_the_slint_file() {
        assert!(HUD.contains("background: bar.color"));
    }

    #[test]
    fn slint_window_size_matches_hud_size() {
        use hud_core::config::{HUD_BASE_HEIGHT, HUD_SIZE};
        assert!(HUD.contains(&format!("width: {}px;", HUD_SIZE.width)));
        // HUD_SIZE is the tallest layout (every provider listed); the window follows what is listed.
        assert!(HUD.contains("height: root.others-y + root.others-h + 22px;"));
        assert!(HUD.contains(&format!("{}px;", HUD_BASE_HEIGHT - 22)));
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
        assert!(MINIBAR.contains("root.vertical ? 6px : 30px + i * 68px"));
        assert!(MINIBAR.contains("root.vertical ? 30px + i * 40px : 6px"));
        assert!(MINIBAR.contains("root.expand()"));
        assert!(HUD.contains("root.minimize()"));
    }

    #[test]
    fn scanner_screen_uses_theme_fonts_and_asks_before_moving() {
        assert!(!families(SCANNER).is_empty());
        assert!(families(SCANNER)
            .iter()
            .all(|f| matches!(*f, "Theme.label-font" | "Theme.number-font")));
        assert!(SCANNER.contains("Mover para a Lixeira"));
        assert!(SCANNER.contains("root.confirm-remove()"));
        assert!(!SCANNER.contains("Somente leitura"));
        assert!(SCANNER.contains("row.review"));
        assert!(HUD.contains("Scanner"));
    }

    #[test]
    fn hud_pin_icon_toggles_click_through_and_shows_its_state() {
        assert!(HUD.contains("root.pin()"));
        assert!(HUD.contains("root.pinned ? Theme.teal"));
    }
}
