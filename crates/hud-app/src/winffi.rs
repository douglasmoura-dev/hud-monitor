//! The only module allowed to use `unsafe` (Windows FFI).

use hud_core::integration::{Backdrop, IntegrationError, RunKey};
use hud_core::metrics::{Date, DiskIo};
use hud_core::position::Size;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND};
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1};
use windows::Win32::NetworkManagement::IpHelper::{
    IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho, ICMP_ECHO_REPLY,
};
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, IBindCtx, CLSCTX_ALL,
    COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
    PdhGetFormattedCounterValue, PdhOpenQueryW, PDH_FMT_COUNTERVALUE, PDH_FMT_COUNTERVALUE_ITEM_W,
    PDH_FMT_DOUBLE, PDH_FMT_LARGE, PDH_HCOUNTER, PDH_HQUERY,
};
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
    KEY_SET_VALUE, REG_SZ,
};
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CONTROL};
use windows::Win32::UI::Shell::{
    FileOperation, IFileOperation, IFileOperationProgressSink, IShellItem,
    SHCreateItemFromParsingName, SHEmptyRecycleBinW, SHQueryRecycleBinW, FILEOPERATION_FLAGS,
    FOFX_RECYCLEONDELETE, FOF_ALLOWUNDO, FOF_NOERRORUI, FOF_SILENT, FOF_WANTNUKEWARNING,
    SHERB_NOCONFIRMATION, SHERB_NOPROGRESSUI, SHERB_NOSOUND, SHQUERYRBINFO,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, SM_CXSCREEN, SM_CYSCREEN,
};

const PDH_OK: u32 = 0;
const PDH_MORE_DATA: u32 = 0x800007D2;
const INTEL_VENDOR_ID: u32 = 0x8086;
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

/// Whether this process runs elevated (administrator).
pub fn is_elevated() -> bool {
    let mut token = HANDLE::default();
    let mut elevation = TOKEN_ELEVATION::default();
    let mut returned = 0u32;
    // SAFETY: `token` and `elevation` are live locals sized as passed; the token handle is
    // closed before returning, and GetCurrentProcess returns a pseudo-handle that needs none.
    unsafe {
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(std::ptr::from_mut(&mut elevation).cast()),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        )
        .is_ok();
        let _ = CloseHandle(token);
        ok && elevation.TokenIsElevated != 0
    }
}

/// Size in bytes and item count of the Recycle Bin on all drives (read-only query).
pub fn recycle_bin_info() -> Result<(u64, u64), String> {
    let mut info = SHQUERYRBINFO {
        cbSize: std::mem::size_of::<SHQUERYRBINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: `info` is a live local with `cbSize` set as the API requires; a null root path
    // means every drive. The call only reads the Recycle Bin state.
    let result = unsafe { SHQueryRecycleBinW(PCWSTR::null(), &mut info) };
    result.map_err(|e| format!("Lixeira indisponível: {e}"))?;
    Ok((
        u64::try_from(info.i64Size).map_err(|e| e.to_string())?,
        u64::try_from(info.i64NumItems).map_err(|e| e.to_string())?,
    ))
}

/// Keeps COM initialised on this thread; uninitialises on drop.
struct ComScope;

impl ComScope {
    fn enter() -> Result<Self, String> {
        // SAFETY: plain per-thread COM initialisation; the matching CoUninitialize is in Drop,
        // and only runs when this call succeeded.
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
        hr.ok().map_err(|e| format!("COM indisponível: {e}"))?;
        Ok(Self)
    }
}

impl Drop for ComScope {
    fn drop(&mut self) {
        // SAFETY: balances the successful CoInitializeEx in `enter` on the same thread.
        unsafe { CoUninitialize() };
    }
}

fn wide_path(path: &std::path::Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

/// Flags that send items to the Recycle Bin and nothing else: undo information is kept and
/// the shell must recycle on delete. There is no "no confirmation" flag, so a delete that
/// would be permanent (item too big for the bin) asks instead of silently destroying data.
fn recycle_flags() -> FILEOPERATION_FLAGS {
    FILEOPERATION_FLAGS(
        FOF_ALLOWUNDO.0
            | FOFX_RECYCLEONDELETE.0
            | FOF_NOERRORUI.0
            | FOF_SILENT.0
            | FOF_WANTNUKEWARNING.0,
    )
}

/// Sends every path to the Recycle Bin with one `IFileOperation`. Items that cannot be queued
/// (missing, no access) are left alone; the caller checks which ones are still on disk.
pub fn recycle_paths(paths: &[PathBuf]) -> Result<(), String> {
    let _com = ComScope::enter()?;
    // SAFETY: COM is initialised on this thread (`_com`). `operation` and every `IShellItem` are
    // owned smart pointers released on drop; each `wide` buffer is NUL-terminated and outlives
    // the call that reads it. Only DeleteItem with recycle flags is queued, never a permanent
    // delete.
    unsafe {
        let operation: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_ALL)
            .map_err(|e| format!("IFileOperation indisponível: {e}"))?;
        operation
            .SetOperationFlags(recycle_flags())
            .map_err(|e| format!("flags da operação: {e}"))?;
        for path in paths {
            let wide = wide_path(path);
            let item: Result<IShellItem, _> =
                SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None::<&IBindCtx>);
            if let Ok(item) = item {
                let _ = operation.DeleteItem(&item, None::<&IFileOperationProgressSink>);
            }
        }
        operation
            .PerformOperations()
            .map_err(|e| format!("Lixeira recusou a operação: {e}"))
    }
}

/// Empties the Recycle Bin of every drive without confirmation or progress UI. Only call it
/// after the user confirmed the "Lixeira" category.
pub fn empty_recycle_bin() -> Result<(), String> {
    // SAFETY: a null window and a null root path mean "all drives"; no pointers are retained.
    unsafe {
        SHEmptyRecycleBinW(
            None,
            PCWSTR::null(),
            SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND,
        )
    }
    .map_err(|e| format!("Lixeira não pôde ser esvaziada: {e}"))
}

/// Current local date and seconds elapsed since local midnight.
pub fn local_now() -> (Date, u32) {
    // SAFETY: GetLocalTime takes no arguments and returns a plain struct by value.
    let t = unsafe { GetLocalTime() };
    let date = Date {
        year: i32::from(t.wYear),
        month: u32::from(t.wMonth),
        day: u32::from(t.wDay),
    };
    let secs = u32::from(t.wHour) * 3600 + u32::from(t.wMinute) * 60 + u32::from(t.wSecond);
    (date, secs)
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

/// PDH query over a wildcard counter path such as `\GPU Engine(*)\Utilization Percentage`.
pub struct CounterArray {
    query: PDH_HQUERY,
    counter: PDH_HCOUNTER,
}

impl CounterArray {
    /// `None` when PDH or the counter path is unavailable.
    pub fn open(path: &str) -> Option<Self> {
        let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
        let mut query = PDH_HQUERY::default();
        let mut counter = PDH_HCOUNTER::default();
        // SAFETY: `wide` is NUL-terminated and outlives the calls; the out pointers refer to
        // live locals; the query is closed in Drop or on failure here.
        unsafe {
            if PdhOpenQueryW(None, 0, &mut query) != PDH_OK {
                return None;
            }
            let ok = PdhAddEnglishCounterW(query, PCWSTR(wide.as_ptr()), 0, &mut counter) == PDH_OK
                && PdhCollectQueryData(query) == PDH_OK;
            if !ok {
                PdhCloseQuery(query);
                return None;
            }
        }
        Some(Self { query, counter })
    }

    /// `(instance name, value)` for every instance; `None` on any PDH error.
    pub fn sample(&mut self) -> Option<Vec<(String, f64)>> {
        // SAFETY: the query and counter are valid (see `open`). The item buffer is
        // 8-byte aligned and sized by the first call as PDH documents; names point into it
        // and are read before the buffer is dropped.
        unsafe {
            if PdhCollectQueryData(self.query) != PDH_OK {
                return None;
            }
            let (mut bytes, mut count) = (0u32, 0u32);
            let status = PdhGetFormattedCounterArrayW(
                self.counter,
                PDH_FMT_DOUBLE,
                &mut bytes,
                &mut count,
                None,
            );
            if status != PDH_MORE_DATA {
                return None;
            }
            let mut buf = vec![0u64; (bytes as usize).div_ceil(8)];
            let items = buf.as_mut_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>();
            let status = PdhGetFormattedCounterArrayW(
                self.counter,
                PDH_FMT_DOUBLE,
                &mut bytes,
                &mut count,
                Some(items),
            );
            if status != PDH_OK {
                return None;
            }
            let list = std::slice::from_raw_parts(items, count as usize);
            Some(
                list.iter()
                    .filter(|i| i.FmtValue.CStatus == PDH_OK)
                    .filter_map(|i| {
                        let name = i.szName.to_string().ok()?;
                        Some((name, i.FmtValue.Anonymous.doubleValue))
                    })
                    .collect(),
            )
        }
    }
}

impl Drop for CounterArray {
    fn drop(&mut self) {
        // SAFETY: the query handle is valid and closed exactly once here.
        unsafe {
            PdhCloseQuery(self.query);
        }
    }
}

/// `luid_0xHHHHHHHH_0xLLLLLLLL` tag (as in `\GPU Engine` instance names) of the first Intel adapter.
pub fn intel_adapter_luid() -> Option<String> {
    // SAFETY: plain COM calls on interfaces owned by this scope; the descriptor is a local
    // out parameter.
    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory1().ok()?;
        let mut index = 0;
        while let Ok(adapter) = factory.EnumAdapters1(index) {
            let desc = adapter.GetDesc1().ok()?;
            if desc.VendorId == INTEL_VENDOR_ID {
                let luid = desc.AdapterLuid;
                return Some(format!(
                    "luid_0x{:08X}_0x{:08X}",
                    luid.HighPart as u32, luid.LowPart
                ));
            }
            index += 1;
        }
        None
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

fn hwnd(raw: isize) -> HWND {
    HWND(raw as *mut std::ffi::c_void)
}

/// True while either Ctrl key is physically down.
pub fn ctrl_down() -> bool {
    // SAFETY: GetAsyncKeyState takes a plain virtual-key code and has no preconditions.
    unsafe { GetAsyncKeyState(i32::from(VK_CONTROL.0)) < 0 }
}

/// Extended window style of a live window.
pub fn ex_style(raw: isize) -> u32 {
    // SAFETY: reads a style word of a window handle; an invalid handle only yields 0.
    unsafe { GetWindowLongPtrW(hwnd(raw), GWL_EXSTYLE) as u32 }
}

/// Replaces the extended window style of a live window.
pub fn set_ex_style(raw: isize, style: u32) {
    // SAFETY: writes a style word of a window handle owned by this process; an invalid
    // handle makes the call fail without side effects.
    unsafe {
        SetWindowLongPtrW(hwnd(raw), GWL_EXSTYLE, style as isize);
    }
}

#[repr(C)]
struct AccentPolicy {
    state: u32,
    flags: u32,
    gradient_color: u32,
    animation_id: u32,
}

#[repr(C)]
struct CompositionAttribute {
    attribute: u32,
    data: *mut AccentPolicy,
    size: usize,
}

type SetCompositionAttribute =
    unsafe extern "system" fn(HWND, *mut CompositionAttribute) -> windows::core::BOOL;

const WCA_ACCENT_POLICY: u32 = 19;
const ACCENT_ENABLE_ACRYLICBLURBEHIND: u32 = 4;

/// Acrylic backdrop of one window via the undocumented `SetWindowCompositionAttribute`.
pub struct WindowBackdrop(pub isize);

impl Backdrop for WindowBackdrop {
    fn enable_acrylic(&mut self, tint_abgr: u32) -> Result<(), IntegrationError> {
        let fail = |what: &str| IntegrationError(what.to_owned());
        // SAFETY: the function pointer comes from user32's export of the documented-by-use
        // `SetWindowCompositionAttribute` with its known signature; the policy and attribute
        // structs are locals that outlive the call.
        unsafe {
            let user32 = GetModuleHandleW(w!("user32.dll")).map_err(|e| fail(&e.to_string()))?;
            let proc = GetProcAddress(user32, windows::core::s!("SetWindowCompositionAttribute"))
                .ok_or_else(|| fail("SetWindowCompositionAttribute not found"))?;
            let set: SetCompositionAttribute = std::mem::transmute(proc);
            let mut policy = AccentPolicy {
                state: ACCENT_ENABLE_ACRYLICBLURBEHIND,
                flags: 0,
                gradient_color: tint_abgr,
                animation_id: 0,
            };
            let mut data = CompositionAttribute {
                attribute: WCA_ACCENT_POLICY,
                data: &mut policy,
                size: std::mem::size_of::<AccentPolicy>(),
            };
            if set(hwnd(self.0), &mut data).as_bool() {
                Ok(())
            } else {
                Err(fail("SetWindowCompositionAttribute failed"))
            }
        }
    }
}

const RUN_KEY: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");

/// `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.
pub struct RegistryRun;

impl RegistryRun {
    fn open() -> Result<HKEY, IntegrationError> {
        let mut key = HKEY::default();
        // SAFETY: the out pointer is a live local; the key is closed by the caller.
        let status =
            unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, None, KEY_SET_VALUE, &mut key) };
        status
            .ok()
            .map(|()| key)
            .map_err(|e| IntegrationError(e.to_string()))
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

impl RunKey for RegistryRun {
    fn set(&mut self, name: &str, command: &str) -> Result<(), IntegrationError> {
        let key = Self::open()?;
        let (name, data) = (wide(name), wide(command));
        // SAFETY: `name` is NUL-terminated; the data slice is the UTF-16 bytes including the
        // terminator as REG_SZ requires; both outlive the call; the key is open and closed here.
        let status = unsafe {
            let bytes = std::slice::from_raw_parts(data.as_ptr().cast::<u8>(), data.len() * 2);
            let status = RegSetValueExW(key, PCWSTR(name.as_ptr()), None, REG_SZ, Some(bytes));
            let _ = RegCloseKey(key);
            status
        };
        status.ok().map_err(|e| IntegrationError(e.to_string()))
    }

    fn remove(&mut self, name: &str) -> Result<(), IntegrationError> {
        const NOT_FOUND: u32 = 2;
        let key = Self::open()?;
        let name = wide(name);
        // SAFETY: `name` is NUL-terminated and outlives the call; the key is open and closed here.
        let status = unsafe {
            let status = RegDeleteValueW(key, PCWSTR(name.as_ptr()));
            let _ = RegCloseKey(key);
            status
        };
        if status.0 == NOT_FOUND {
            return Ok(());
        }
        status.ok().map_err(|e| IntegrationError(e.to_string()))
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
    fn missing_counter_path_is_none() {
        assert!(super::CounterArray::open(r"\Nada(*)\Nada").is_none());
    }

    #[test]
    fn local_date_is_plausible() {
        let (d, secs) = super::local_now();
        assert!(secs < 86_400);
        assert!(d.year >= 2024 && (1..=12).contains(&d.month) && (1..=31).contains(&d.day));
    }
}
