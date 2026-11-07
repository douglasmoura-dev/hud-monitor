//! The only module allowed to use `unsafe` (Windows FFI).

use hud_core::metrics::{Date, DiskIo};
use hud_core::position::Size;
use windows::core::w;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::NetworkManagement::IpHelper::{
    IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho, ICMP_ECHO_REPLY,
};
use windows::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterValue,
    PdhOpenQueryW, PDH_FMT_COUNTERVALUE, PDH_FMT_LARGE, PDH_HCOUNTER, PDH_HQUERY,
};
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

const PDH_OK: u32 = 0;
const ICMP_REPLY_WORDS: usize = 32;

/// Primary monitor size in physical pixels.
pub fn screen_size() -> Size {
    // SAFETY: GetSystemMetrics takes a plain integer index, has no pointer args
    // and no preconditions; it cannot violate memory safety.
    unsafe {
        Size {
            width: GetSystemMetrics(SM_CXSCREEN),
            height: GetSystemMetrics(SM_CYSCREEN),
        }
    }
}

/// Current local calendar date.
pub fn local_date() -> Date {
    // SAFETY: GetLocalTime takes no arguments and returns a plain struct by value.
    let t = unsafe { GetLocalTime() };
    Date {
        year: i32::from(t.wYear),
        month: u32::from(t.wMonth),
        day: u32::from(t.wDay),
    }
}

/// PDH query for `\PhysicalDisk(_Total)` read/write bytes per second.
pub struct DiskCounters {
    query: PDH_HQUERY,
    read: PDH_HCOUNTER,
    write: PDH_HCOUNTER,
}

impl DiskCounters {
    /// `None` when PDH or the counters are unavailable.
    pub fn open() -> Option<Self> {
        let mut query = PDH_HQUERY::default();
        let mut read = PDH_HCOUNTER::default();
        let mut write = PDH_HCOUNTER::default();
        // SAFETY: every pointer refers to a live local; the query handle is closed in Drop
        // and the counter handles are only used while the query is open.
        unsafe {
            if PdhOpenQueryW(None, 0, &mut query) != PDH_OK {
                return None;
            }
            let added = PdhAddEnglishCounterW(
                query,
                w!("\\PhysicalDisk(_Total)\\Disk Read Bytes/sec"),
                0,
                &mut read,
            ) == PDH_OK
                && PdhAddEnglishCounterW(
                    query,
                    w!("\\PhysicalDisk(_Total)\\Disk Write Bytes/sec"),
                    0,
                    &mut write,
                ) == PDH_OK
                && PdhCollectQueryData(query) == PDH_OK;
            if !added {
                PdhCloseQuery(query);
                return None;
            }
        }
        Some(Self { query, read, write })
    }

    /// Rate since the previous call (rate counters need two samples).
    pub fn sample(&mut self) -> Option<DiskIo> {
        // SAFETY: the query and counter handles were created in `open` and are still valid.
        unsafe {
            if PdhCollectQueryData(self.query) != PDH_OK {
                return None;
            }
            Some(DiskIo {
                read_per_sec: counter_value(self.read)?,
                write_per_sec: counter_value(self.write)?,
            })
        }
    }
}

/// SAFETY: caller passes a counter handle belonging to an open query.
unsafe fn counter_value(counter: PDH_HCOUNTER) -> Option<u64> {
    let mut value = PDH_FMT_COUNTERVALUE::default();
    if PdhGetFormattedCounterValue(counter, PDH_FMT_LARGE, None, &mut value) != PDH_OK
        || value.CStatus != PDH_OK
    {
        return None;
    }
    u64::try_from(value.Anonymous.largeValue).ok()
}

impl Drop for DiskCounters {
    fn drop(&mut self) {
        // SAFETY: the query handle is valid and closed exactly once here.
        unsafe {
            PdhCloseQuery(self.query);
        }
    }
}

/// ICMP echo handle (`IcmpSendEcho`).
pub struct Icmp(HANDLE);

impl Icmp {
    pub fn open() -> Option<Self> {
        // SAFETY: IcmpCreateFile has no arguments; the handle is closed in Drop.
        unsafe { IcmpCreateFile().ok().map(Self) }
    }

    /// Round-trip time in ms, `None` on timeout or error.
    pub fn ping_ms(&self, ip: [u8; 4], timeout_ms: u32) -> Option<u32> {
        let payload = [0u8; 8];
        let mut reply = [0u64; ICMP_REPLY_WORDS];
        // SAFETY: the reply buffer is 8-byte aligned, 256 bytes, larger than one
        // ICMP_ECHO_REPLY plus the 8 payload bytes IcmpSendEcho documents; the request
        // buffer outlives the call; the handle is valid until Drop.
        unsafe {
            let n = IcmpSendEcho(
                self.0,
                u32::from_ne_bytes(ip),
                payload.as_ptr().cast(),
                payload.len() as u16,
                None,
                reply.as_mut_ptr().cast(),
                std::mem::size_of_val(&reply) as u32,
                timeout_ms,
            );
            if n == 0 {
                return None;
            }
            let echo = &*reply.as_ptr().cast::<ICMP_ECHO_REPLY>();
            (echo.Status == 0).then_some(echo.RoundTripTime)
        }
    }
}

impl Drop for Icmp {
    fn drop(&mut self) {
        // SAFETY: the handle came from IcmpCreateFile and is closed exactly once.
        unsafe {
            let _ = IcmpCloseHandle(self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn screen_has_positive_size() {
        let s = super::screen_size();
        assert!(s.width > 0 && s.height > 0);
    }

    #[test]
    fn local_date_is_plausible() {
        let d = super::local_date();
        assert!(d.year >= 2024 && (1..=12).contains(&d.month) && (1..=31).contains(&d.day));
    }
}
