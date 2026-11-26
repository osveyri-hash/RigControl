//! MainWindow.Memory.cs: сколько памяти занимает программа и как отдать лишнее системе.

/// Частный рабочий набор — то же число, что «Память» в диспетчере задач.
#[cfg(windows)]
pub fn usage_bytes() -> u64 {
    #[repr(C)]
    #[allow(non_snake_case)]
    struct Counters {
        Size: u32,
        PageFaultCount: u32,
        PeakWorkingSetSize: usize,
        WorkingSetSize: usize,
        QuotaPeakPagedPoolUsage: usize,
        QuotaPagedPoolUsage: usize,
        QuotaPeakNonPagedPoolUsage: usize,
        QuotaNonPagedPoolUsage: usize,
        PagefileUsage: usize,
        PeakPagefileUsage: usize,
        PrivateUsage: usize,
        PrivateWorkingSetSize: usize,
        SharedCommitUsage: u64,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> *mut std::ffi::c_void;
        fn K32GetProcessMemoryInfo(process: *mut std::ffi::c_void, counters: *mut Counters, size: u32) -> i32;
    }
    unsafe {
        let mut c: Counters = std::mem::zeroed();
        c.Size = std::mem::size_of::<Counters>() as u32;
        if K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.Size) != 0 {
            return if c.PrivateWorkingSetSize > 0 { c.PrivateWorkingSetSize as u64 } else { c.WorkingSetSize as u64 };
        }
    }
    0
}

#[cfg(not(windows))]
pub fn usage_bytes() -> u64 {
    std::fs::read_to_string("/proc/self/statm")
        .ok()
        .and_then(|s| s.split_whitespace().nth(1).and_then(|v| v.parse::<u64>().ok()))
        .map(|pages| pages * 4096)
        .unwrap_or(0)
}

/// Подпись «ОЗУ: N МБ» (округление как в C#).
pub fn label() -> String {
    let b = usage_bytes();
    format!("ОЗУ: {} МБ", ((b + 512 * 1024) / (1024 * 1024)).max(1))
}

/// Отдать системе страницы, которые сейчас не нужны (SetProcessWorkingSetSize(-1, -1)).
/// В Rust нет сборщика мусора, так что это весь TrimMemory.
#[cfg(windows)]
pub fn trim() {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> *mut std::ffi::c_void;
        fn SetProcessWorkingSetSize(process: *mut std::ffi::c_void, min: isize, max: isize) -> i32;
    }
    unsafe {
        SetProcessWorkingSetSize(GetCurrentProcess(), -1, -1);
    }
}

#[cfg(not(windows))]
pub fn trim() {}

#[cfg(test)]
mod tests {
    #[test]
    fn label_format() {
        let l = super::label();
        assert!(l.starts_with("ОЗУ: ") && l.ends_with(" МБ"));
    }
}
