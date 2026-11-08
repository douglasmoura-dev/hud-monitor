//! Trait adapters over the Windows FFI module.

use crate::winffi::{DiskCounters, Icmp};
use hud_core::metrics::{DiskIo, DiskIoSource, PingSource};

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
