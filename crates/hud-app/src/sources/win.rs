//! Trait adapters over the Windows FFI module.

use crate::winffi::{intel_adapter_luid, CounterArray, DiskCounters, Icmp};
use hud_core::metrics::{DiskIo, DiskIoSource, PingSource};
use hud_core::sensors::{
    adapter_usage, IntelGpuSource, NvidiaError, NvidiaReading, NvidiaSource, ThermalSource,
};
use nvml_wrapper::{enum_wrappers::device::TemperatureSensor, Nvml};

const PING_TARGET: [u8; 4] = [1, 1, 1, 1];
const PING_TIMEOUT_MS: u32 = 1000;

pub struct PdhDiskIo(Option<DiskCounters>);

impl PdhDiskIo {
    pub fn new() -> Self {
        Self(DiskCounters::open())
    }
}

impl DiskIoSource for PdhDiskIo {
    fn sample(&mut self) -> Option<DiskIo> {
        self.0.as_mut()?.sample()
    }
}

pub struct IcmpPing(Option<Icmp>);

impl IcmpPing {
    pub fn new() -> Self {
        Self(Icmp::open())
    }
}

impl PingSource for IcmpPing {
    fn ping_ms(&mut self) -> Option<u32> {
        self.0.as_ref()?.ping_ms(PING_TARGET, PING_TIMEOUT_MS)
    }
}

pub struct PdhThermal(Option<CounterArray>);

impl PdhThermal {
    pub fn new() -> Self {
        Self(CounterArray::open(
            r"\Thermal Zone Information(*)\Temperature",
        ))
    }
}

impl ThermalSource for PdhThermal {
    fn zones_kelvin(&mut self) -> Vec<f64> {
        self.0
            .as_mut()
            .and_then(CounterArray::sample)
            .map_or_else(Vec::new, |zones| {
                zones.into_iter().map(|(_, k)| k).collect()
            })
    }
}

/// Counter path limited to the engines of one adapter, so PDH does not enumerate every process.
fn intel_engine_path(luid: &str) -> String {
    format!(
        r"\GPU Engine(*{}*)\Utilization Percentage",
        luid.to_ascii_lowercase()
    )
}

pub struct PdhIntelGpu(Option<(CounterArray, String)>);

impl PdhIntelGpu {
    pub fn new() -> Self {
        Self(intel_adapter_luid().and_then(|luid| {
            let path = intel_engine_path(&luid);
            CounterArray::open(&path).map(|counters| (counters, luid))
        }))
    }
}

impl IntelGpuSource for PdhIntelGpu {
    fn usage_percent(&mut self) -> Option<f32> {
        let (counters, luid) = self.0.as_mut()?;
        adapter_usage(&counters.sample()?, luid)
    }
}

/// NVML loaded at runtime from `nvml.dll`; absent driver means `NoDriver`.
pub struct NvmlSource(Option<Nvml>);

impl NvmlSource {
    pub fn new() -> Self {
        Self(Nvml::init().ok())
    }
}

impl NvidiaSource for NvmlSource {
    fn read(&mut self) -> Result<NvidiaReading, NvidiaError> {
        let nvml = self.0.as_ref().ok_or(NvidiaError::NoDriver)?;
        let device = nvml.device_by_index(0).map_err(|_| NvidiaError::Failed)?;
        let usage = device
            .utilization_rates()
            .map_err(|_| NvidiaError::Failed)?;
        // Old drivers lack nvmlDeviceGetMemoryInfo_v2; only VRAM is lost then.
        let vram = device.memory_info().ok().map(|m| (m.used, m.total));
        let temp_c = device
            .temperature(TemperatureSensor::Gpu)
            .map_err(|_| NvidiaError::Failed)?;
        Ok(NvidiaReading {
            usage_percent: usage.gpu,
            vram,
            temp_c,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_path_is_limited_to_the_adapter() {
        assert_eq!(
            intel_engine_path("luid_0x00000000_0x0000D0A5"),
            r"\GPU Engine(*luid_0x00000000_0x0000d0a5*)\Utilization Percentage"
        );
    }

    #[test]
    fn narrowed_counter_still_reads_the_intel_adapter_when_present() {
        let Some(luid) = intel_adapter_luid() else {
            return;
        };
        let mut gpu = PdhIntelGpu::new();
        assert!(gpu.0.as_ref().is_some_and(|(_, l)| *l == luid));
        std::thread::sleep(std::time::Duration::from_millis(1100));
        assert!(gpu.usage_percent().is_some());
    }
}
