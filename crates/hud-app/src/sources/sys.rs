//! `sysinfo`-backed source. The `System` is created once; each call refreshes only what it needs.

use crate::winffi::NetCounters;
use hud_core::metrics::{ProcessMem, SystemSource};
use std::{
    path::Path,
    time::{Duration, Instant},
};
use sysinfo::{
    CpuRefreshKind, DiskRefreshKind, Disks, MemoryRefreshKind, ProcessRefreshKind,
    ProcessesToUpdate, System,
};

const PROCESS_REFRESH: Duration = Duration::from_secs(10);
const TOP_PROCESSES: usize = 3;

#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error(transparent)]
    Sysinfo(#[from] sysinfo::Error),
    #[error("network interfaces could not be listed")]
    Network,
}

pub struct SysinfoSource {
    system: System,
    disks: Disks,
    net: NetCounters,
    top: Vec<ProcessMem>,
    top_at: Option<Instant>,
}

impl SysinfoSource {
    pub fn new() -> Result<Self, SourceError> {
        let mut system = System::new()?;
        system.refresh_cpu_specifics(CpuRefreshKind::everything());
        Ok(Self {
            system,
            disks: Disks::new_with_refreshed_list_specifics(
                DiskRefreshKind::nothing().with_storage(),
            )?,
            net: NetCounters::open().ok_or(SourceError::Network)?,
            top: Vec::new(),
            top_at: None,
        })
    }

    fn refresh_top_processes(&mut self) {
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_memory(),
        );
        let mut all: Vec<ProcessMem> = self
            .system
            .processes()
            .values()
            .map(|p| ProcessMem {
                name: p.name().to_string_lossy().into_owned(),
                bytes: p.memory(),
            })
            .collect();
        all.sort_by_key(|p| std::cmp::Reverse(p.bytes));
        all.truncate(TOP_PROCESSES);
        self.top = all;
    }
}

impl SystemSource for SysinfoSource {
    fn cpu_threads(&mut self) -> Vec<f32> {
        self.system
            .refresh_cpu_specifics(CpuRefreshKind::nothing().with_usage());
        self.system.cpus().iter().map(|c| c.usage()).collect()
    }

    fn cpu_freq_mhz(&mut self) -> Option<u64> {
        self.system
            .refresh_cpu_specifics(CpuRefreshKind::nothing().with_frequency());
        self.system.cpus().first().map(|c| c.frequency())
    }

    fn memory(&mut self) -> (u64, u64) {
        self.system
            .refresh_memory_specifics(MemoryRefreshKind::nothing().with_ram());
        (self.system.used_memory(), self.system.total_memory())
    }

    fn top_processes(&mut self) -> Vec<ProcessMem> {
        let due = self.top_at.is_none_or(|t| t.elapsed() >= PROCESS_REFRESH);
        if due {
            self.refresh_top_processes();
            self.top_at = Some(Instant::now());
        }
        self.top.clone()
    }

    fn physical_cores(&mut self) -> Option<usize> {
        System::physical_core_count().ok()
    }

    fn system_drive(&self) -> Option<String> {
        std::env::var("SystemDrive").ok()
    }

    fn disk_usage(&mut self) -> Option<(u64, u64)> {
        self.disks
            .refresh_specifics(false, DiskRefreshKind::nothing().with_storage());
        let drive = format!("{}\\", self.system_drive()?);
        let disk = self
            .disks
            .iter()
            .find(|d| d.mount_point() == Path::new(&drive))?;
        let total = disk.total_space();
        Some((total.saturating_sub(disk.available_space()), total))
    }

    fn net_counters(&mut self) -> (u64, u64) {
        self.net.totals()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_real_values_from_this_machine() {
        let mut s = SysinfoSource::new().unwrap();
        let (used, total) = s.memory();
        assert!(total > 0 && used <= total);
        assert!(!s.cpu_threads().is_empty());
        let top = s.top_processes();
        assert!(!top.is_empty() && top.len() <= TOP_PROCESSES);
        assert!(top.windows(2).all(|w| w[0].bytes >= w[1].bytes));
    }
}
