//! Maps the core view models onto the generated Slint types. No decisions are made here.

use super::{Bar, ChipData, Field, Hud, Limit, MiniBar, PresenceData};
use hud_core::dashboard::DashboardView;
use hud_core::view;
use std::rc::Rc;

pub fn field(f: &view::Field) -> Field {
    Field {
        text: f.text.as_str().into(),
        tip: f.tip.as_str().into(),
        level: f.level as i32,
    }
}

pub fn bar(b: &view::CoreBar) -> Bar {
    let [_, r, g, bl] = b.color.to_be_bytes();
    Bar {
        percent: b.percent,
        color: slint::Color::from_rgb_u8(r, g, bl),
    }
}

pub fn limit(l: &view::Limit) -> Limit {
    Limit {
        pct: field(&l.pct),
        reset: l.reset.as_str().into(),
        percent: l.percent,
    }
}

fn model<T: Clone + 'static>(rows: Vec<T>) -> slint::ModelRc<T> {
    Rc::new(slint::VecModel::from(rows)).into()
}

fn chip_model(chips: &[view::Chip]) -> slint::ModelRc<ChipData> {
    model(
        chips
            .iter()
            .map(|c| ChipData {
                label: c.label.into(),
                field: field(&c.value),
            })
            .collect(),
    )
}

fn show_hud(hud: &Hud, v: &view::HudView) {
    hud.set_cpu_total(field(&v.cpu_total));
    hud.set_cpu_freq(field(&v.cpu_freq));
    hud.set_cpu_temp(field(&v.cpu_temp));
    hud.set_cpu_cores(field(&v.cpu_cores));
    hud.set_core_bars(model(v.core_bars.iter().map(bar).collect()));
    hud.set_ram(field(&v.ram));
    hud.set_ram_percent(v.ram_percent);
    hud.set_ram_top(field(&v.ram_top));
    hud.set_gpu_intel(field(&v.gpu_intel));
    hud.set_gpu_nvidia(field(&v.gpu_nvidia));
    hud.set_gpu_vram(field(&v.gpu_vram));
    hud.set_disk(field(&v.disk));
    hud.set_disk_percent(v.disk_percent);
    hud.set_disk_io(field(&v.disk_io));
    hud.set_net_down(field(&v.net_down));
    hud.set_net_up(field(&v.net_up));
    hud.set_net_today(field(&v.net_today));
    hud.set_ping(field(&v.ping));
}

fn show_providers(hud: &Hud, v: &DashboardView) {
    hud.set_claude_plan(v.claude.plan.as_str().into());
    hud.set_claude_5h(limit(&v.claude.five));
    hud.set_claude_7d(limit(&v.claude.week));
    hud.set_codex_listed(v.codex_listed);
    hud.set_codex_5h(limit(&v.codex.five));
    hud.set_codex_7d(limit(&v.codex.week));
    hud.set_gemini_listed(v.gemini_listed);
    hud.set_gemini_day(limit(&v.gemini));
    hud.set_cursor_listed(v.cursor_listed);
    hud.set_cursor_month(limit(&v.cursor));
    let rows = v
        .presence
        .iter()
        .map(|r| PresenceData {
            name: r.name.into(),
            status: r.status.into(),
        })
        .collect();
    hud.set_presence(model(rows));
}

pub fn show_dashboard(hud: &Hud, minibar: &MiniBar, v: &DashboardView) {
    show_hud(hud, &v.hud);
    show_providers(hud, v);
    minibar.set_chips(chip_model(&v.chips));
}

/// `HH:MM` from seconds since local midnight.
pub fn clock_text(seconds_since_midnight: u32) -> String {
    format!(
        "{:02}:{:02}",
        seconds_since_midnight / 3600,
        seconds_since_midnight / 60 % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use hud_core::alerts::Level;

    #[test]
    fn field_carries_text_tip_and_level() {
        let f = field(&view::Field {
            text: "42%".into(),
            tip: "why".into(),
            level: Level::Crit,
        });
        assert_eq!((f.text.as_str(), f.tip.as_str()), ("42%", "why"));
        assert_eq!(f.level, Level::Crit as i32);
    }

    #[test]
    fn bar_color_unpacks_the_rgb_bytes() {
        let b = bar(&view::CoreBar {
            percent: 50.0,
            color: 0x00E2_4B4A,
        });
        assert_eq!(b.percent, 50.0);
        assert_eq!(b.color, slint::Color::from_rgb_u8(0xE2, 0x4B, 0x4A));
    }

    #[test]
    fn limit_keeps_the_reset_text_and_percent() {
        let l = limit(&view::Limit {
            pct: view::Field::default(),
            reset: "1h48".into(),
            percent: 12.5,
        });
        assert_eq!((l.reset.as_str(), l.percent), ("1h48", 12.5));
    }

    #[test]
    fn clock_is_zero_padded_hours_and_minutes() {
        assert_eq!(clock_text(0), "00:00");
        assert_eq!(clock_text(9 * 3600 + 5 * 60 + 59), "09:05");
        assert_eq!(clock_text(23 * 3600 + 59 * 60), "23:59");
    }
}
