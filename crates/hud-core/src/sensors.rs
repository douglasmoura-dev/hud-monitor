//! Temperature and GPU metrics built from abstract sensor sources.

use crate::metrics::Metric;

pub const THERMAL_UNAVAILABLE: &str = "sensor indisponível";
pub const NO_NVIDIA_DRIVER: &str = "sem driver NVIDIA";
pub const GPU_IDLE: &str = "GPU em repouso";
pub const VRAM_UNAVAILABLE: &str = "VRAM indisponível neste driver";
pub const NVIDIA_FAILED: &str = "leitura NVIDIA falhou";
pub const INTEL_GPU_UNAVAILABLE: &str = "contador GPU Intel indisponível";
const KELVIN_OFFSET: f64 = 273.15;

#[derive(Debug, Clone, PartialEq)]
pub struct ThermalMetrics {
    pub cpu_c: Metric<i32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NvidiaMetrics {
    pub usage_percent: Metric<u32>,
    /// `(used, total)` bytes.
    pub vram: Metric<(u64, u64)>,
    pub temp_c: Metric<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GpuMetrics {
    pub nvidia: NvidiaMetrics,
    pub intel_usage_percent: Metric<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sensors {
    pub thermal: ThermalMetrics,
    pub gpu: GpuMetrics,
}

impl Sensors {
    /// Every sensor `Unavailable(reason)`.
    pub fn unavailable(reason: &str) -> Self {
        Self {
            thermal: ThermalMetrics {
                cpu_c: Metric::unavailable(reason),
            },
            gpu: GpuMetrics {
                nvidia: NvidiaMetrics::unavailable(reason),
                intel_usage_percent: Metric::unavailable(reason),
            },
        }
    }
}

impl NvidiaMetrics {
    fn unavailable(reason: &str) -> Self {
        Self {
            usage_percent: Metric::unavailable(reason),
            vram: Metric::unavailable(reason),
            temp_c: Metric::unavailable(reason),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NvidiaReading {
    pub usage_percent: u32,
    /// `(used, total)` bytes; `None` when the driver is too old to report it.
    pub vram: Option<(u64, u64)>,
    pub temp_c: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NvidiaError {
    #[error("NVML not available")]
    NoDriver,
    #[error("NVML query failed")]
    Failed,
}

pub trait ThermalSource {
    /// Temperature of every thermal zone, in Kelvin.
    fn zones_kelvin(&mut self) -> Vec<f64>;
}

pub trait NvidiaSource {
    fn read(&mut self) -> Result<NvidiaReading, NvidiaError>;
}

pub trait IntelGpuSource {
    /// Utilization of the Intel adapter, summed over its engines, in percent.
    fn usage_percent(&mut self) -> Option<f32>;
}

/// Hottest valid zone in °C; zones at or below 0 K are sensor garbage and ignored.
pub fn thermal_metrics(source: &mut dyn ThermalSource) -> ThermalMetrics {
    let hottest = source
        .zones_kelvin()
        .into_iter()
        .filter(|k| k.is_finite() && *k > 0.0)
        .fold(None, |acc: Option<f64>, k| {
            Some(acc.map_or(k, |a| a.max(k)))
        });
    ThermalMetrics {
        cpu_c: hottest.map_or(Metric::unavailable(THERMAL_UNAVAILABLE), |k| {
            Metric::Available((k - KELVIN_OFFSET).round() as i32)
        }),
    }
}

pub fn nvidia_metrics(source: &mut dyn NvidiaSource) -> NvidiaMetrics {
    match source.read() {
        Ok(r) => NvidiaMetrics {
            usage_percent: Metric::Available(r.usage_percent),
            vram: r
                .vram
                .map_or(Metric::unavailable(VRAM_UNAVAILABLE), Metric::Available),
            // NVML reports 0 °C when the GPU is powered down.
            temp_c: if r.temp_c == 0 {
                Metric::unavailable(GPU_IDLE)
            } else {
                Metric::Available(r.temp_c)
            },
        },
        Err(NvidiaError::NoDriver) => NvidiaMetrics::unavailable(NO_NVIDIA_DRIVER),
        Err(NvidiaError::Failed) => NvidiaMetrics::unavailable(NVIDIA_FAILED),
    }
}

pub fn read_sensors(
    thermal: &mut dyn ThermalSource,
    nvidia: &mut dyn NvidiaSource,
    intel: &mut dyn IntelGpuSource,
) -> Sensors {
    Sensors {
        thermal: thermal_metrics(thermal),
        gpu: GpuMetrics {
            nvidia: nvidia_metrics(nvidia),
            intel_usage_percent: intel.usage_percent().map_or(
                Metric::unavailable(INTEL_GPU_UNAVAILABLE),
                Metric::Available,
            ),
        },
    }
}

/// Sums `\GPU Engine(*)` utilization of the instances belonging to one adapter.
/// `luid` is the `luid_0xHHHHHHHH_0xLLLLLLLL` tag of the adapter; `None` if no instance matches.
pub fn adapter_usage(instances: &[(String, f64)], luid: &str) -> Option<f32> {
    let tag = luid.to_ascii_lowercase();
    let mut matching = instances
        .iter()
        .filter(|(name, _)| name.to_ascii_lowercase().contains(&tag))
        .peekable();
    matching.peek()?;
    let sum: f64 = matching.map(|(_, v)| *v).sum();
    Some(sum.clamp(0.0, 100.0) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Zones(Vec<f64>);
    impl ThermalSource for Zones {
        fn zones_kelvin(&mut self) -> Vec<f64> {
            self.0.clone()
        }
    }

    struct Nv(Result<NvidiaReading, NvidiaError>);
    impl NvidiaSource for Nv {
        fn read(&mut self) -> Result<NvidiaReading, NvidiaError> {
            self.0
        }
    }

    struct Intel(Option<f32>);
    impl IntelGpuSource for Intel {
        fn usage_percent(&mut self) -> Option<f32> {
            self.0
        }
    }

    fn reading(temp_c: u32) -> NvidiaReading {
        NvidiaReading {
            usage_percent: 7,
            vram: Some((100, 2000)),
            temp_c,
        }
    }

    #[test]
    fn hottest_zone_in_celsius() {
        let t = thermal_metrics(&mut Zones(vec![354.15, 340.15]));
        assert_eq!(t.cpu_c, Metric::Available(81));
    }

    #[test]
    fn no_zones_is_unavailable_never_zero() {
        let t = thermal_metrics(&mut Zones(vec![]));
        assert_eq!(t.cpu_c, Metric::unavailable("sensor indisponível"));
    }

    #[test]
    fn zero_kelvin_zones_are_ignored() {
        let t = thermal_metrics(&mut Zones(vec![0.0, f64::NAN]));
        assert_eq!(t.cpu_c, Metric::unavailable(THERMAL_UNAVAILABLE));
    }

    #[test]
    fn missing_nvml_marks_nvidia_unavailable() {
        let n = nvidia_metrics(&mut Nv(Err(NvidiaError::NoDriver)));
        assert_eq!(n.temp_c, Metric::unavailable("sem driver NVIDIA"));
        assert_eq!(n.usage_percent, Metric::unavailable("sem driver NVIDIA"));
        assert_eq!(n.vram, Metric::unavailable("sem driver NVIDIA"));
    }

    #[test]
    fn zero_temperature_means_idle() {
        let n = nvidia_metrics(&mut Nv(Ok(reading(0))));
        assert_eq!(n.temp_c, Metric::unavailable("GPU em repouso"));
        assert_eq!(n.usage_percent, Metric::Available(7));
    }

    #[test]
    fn nvidia_values_pass_through() {
        let n = nvidia_metrics(&mut Nv(Ok(reading(55))));
        assert_eq!(n.temp_c, Metric::Available(55));
        assert_eq!(n.vram, Metric::Available((100, 2000)));
    }

    #[test]
    fn missing_vram_does_not_hide_temperature() {
        let r = NvidiaReading {
            vram: None,
            ..reading(60)
        };
        let n = nvidia_metrics(&mut Nv(Ok(r)));
        assert_eq!(n.vram, Metric::unavailable(VRAM_UNAVAILABLE));
        assert_eq!(n.temp_c, Metric::Available(60));
    }

    #[test]
    fn nvml_query_failure_is_unavailable() {
        let n = nvidia_metrics(&mut Nv(Err(NvidiaError::Failed)));
        assert_eq!(n.temp_c, Metric::unavailable(NVIDIA_FAILED));
    }

    #[test]
    fn intel_failure_is_unavailable() {
        let s = read_sensors(
            &mut Zones(vec![300.0]),
            &mut Nv(Err(NvidiaError::NoDriver)),
            &mut Intel(None),
        );
        assert_eq!(
            s.gpu.intel_usage_percent,
            Metric::unavailable(INTEL_GPU_UNAVAILABLE)
        );
        let s = read_sensors(
            &mut Zones(vec![]),
            &mut Nv(Ok(reading(1))),
            &mut Intel(Some(12.5)),
        );
        assert_eq!(s.gpu.intel_usage_percent, Metric::Available(12.5));
    }

    #[test]
    fn adapter_usage_sums_only_that_adapter() {
        let inst = |n: &str, v: f64| (n.to_string(), v);
        let engines = [
            inst(
                "pid_1_luid_0x00000000_0x0000D0A5_phys_0_eng_0_engtype_3D",
                10.0,
            ),
            inst(
                "pid_2_luid_0x00000000_0x0000d0a5_phys_0_eng_1_engtype_Copy",
                5.5,
            ),
            inst(
                "pid_3_luid_0x00000000_0x0000FFFF_phys_0_eng_0_engtype_3D",
                40.0,
            ),
        ];
        assert_eq!(
            adapter_usage(&engines, "luid_0x00000000_0x0000D0A5"),
            Some(15.5)
        );
        assert_eq!(adapter_usage(&engines, "luid_0x00000000_0x00000001"), None);
    }

    #[test]
    fn adapter_usage_is_capped_at_100() {
        let engines = [
            ("luid_0x0_0x1_a".to_string(), 80.0),
            ("luid_0x0_0x1_b".to_string(), 80.0),
        ];
        assert_eq!(adapter_usage(&engines, "luid_0x0_0x1"), Some(100.0));
    }
}
