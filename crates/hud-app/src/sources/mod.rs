//! Real implementations of the `hud-core` source traits.

mod claude;
mod cleanup;
mod day_store;
mod extras;
mod sys;
mod win;

pub use claude::{FileCredentials, JsonlScanner, UreqApi};
pub use cleanup::{env_roots, ShellRecycler, StdFs, WinSystem};
pub use day_store::FileDayStore;
pub use extras::{ollama_up, CursorNoSource, GeminiLogs, PathProbe};
pub use sys::SysinfoSource;
pub use win::{IcmpPing, NvmlSource, PdhDiskIo, PdhIntelGpu, PdhThermal};
