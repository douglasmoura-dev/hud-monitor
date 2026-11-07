//! Pure text formatting for metrics. Units are binary (1 MB = 1024 * 1024 B).

pub const KIB: u64 = 1024;
pub const MIB: u64 = KIB * 1024;
pub const GIB: u64 = MIB * 1024;

fn scaled(value: f64) -> String {
    if value >= 100.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
    }
}

/// Bytes per second as `"4.2 MB/s"`, `"320 KB/s"`, `"12 B/s"`.
pub fn rate(bytes_per_sec: u64) -> String {
    format!("{}/s", size(bytes_per_sec))
}

/// Byte volume with binary units: `"3.1 GB"`, `"512 MB"`, `"12 B"`.
pub fn size(bytes: u64) -> String {
    let b = bytes as f64;
    if bytes >= GIB {
        format!("{} GB", scaled(b / GIB as f64))
    } else if bytes >= MIB {
        format!("{} MB", scaled(b / MIB as f64))
    } else if bytes >= KIB {
        format!("{} KB", scaled(b / KIB as f64))
    } else {
        format!("{bytes} B")
    }
}

/// Byte size as GiB with one decimal, dropping a trailing `.0`.
fn gb(bytes: u64) -> String {
    let s = format!("{:.1}", bytes as f64 / GIB as f64);
    match s.strip_suffix(".0") {
        Some(whole) => whole.to_string(),
        None => s,
    }
}

/// `"11.2/16 GB"`.
pub fn used_total_gb(used: u64, total: u64) -> String {
    format!("{}/{} GB", gb(used), gb(total))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_rate_in_mb() {
        assert_eq!(rate(4_404_019), "4.2 MB/s");
    }

    #[test]
    fn upload_rate_in_kb() {
        assert_eq!(rate(327_680), "320 KB/s");
    }

    #[test]
    fn small_and_huge_rates() {
        assert_eq!(rate(0), "0 B/s");
        assert_eq!(rate(1023), "1023 B/s");
        assert_eq!(rate(3 * GIB), "3.0 GB/s");
    }

    #[test]
    fn ram_text() {
        let used = (11.2 * GIB as f64) as u64;
        assert_eq!(used_total_gb(used, 16 * GIB), "11.2/16 GB");
        assert_eq!(used_total_gb(8 * GIB, 16 * GIB), "8/16 GB");
    }

    #[test]
    fn sizes() {
        assert_eq!(size(512), "512 B");
        assert_eq!(size(3_328_599_654), "3.1 GB");
        assert_eq!(size(200 * MIB), "200 MB");
    }
}
