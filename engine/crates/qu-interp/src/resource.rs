//! This process's own live resource usage, right now -- not the machine
//! (see [`crate::sysinfo`] for that), not a history of it, just what it is
//! at the moment something calls in here.
//!
//! Currently just resident-set size. Backs the `profile_start`/
//! `profile_end` builtins ([`crate::lib`]'s builtin dispatch), and is the
//! same primitive `qu-cli`'s `--profile`/`--max-memory` watchdog uses --
//! moved here (rather than kept private to `qu-cli`) so both call sites
//! share one implementation instead of two copies of the same platform
//! FFI silently drifting apart.
//!
//! No new dependency: hand-rolled FFI to `psapi.dll` on Windows, and a
//! `/proc/self/status` read on Linux -- a dozen-odd lines each, in keeping
//! with this workspace's default of not pulling in a crate for something
//! this small. Anywhere else, [`current_rss_bytes`] returns `None`; a
//! caller that turns `None` into a fabricated number instead of
//! propagating "unavailable" is the actual bug, not this file.

/// Current process resident-set size (physical memory actually mapped in),
/// in bytes -- see this module's doc comment for platform coverage.
#[cfg(windows)]
pub fn current_rss_bytes() -> Option<u64> {
    // Manual FFI instead of a crate dependency (`winapi`/`windows-sys`):
    // psapi's `PROCESS_MEMORY_COUNTERS`/`GetProcessMemoryInfo` ABI has
    // been stable since Windows XP, so hand-declaring it is low-risk.
    #[repr(C)]
    struct ProcessMemoryCounters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }

    #[link(name = "psapi")]
    extern "system" {
        fn GetProcessMemoryInfo(
            process: *mut std::ffi::c_void,
            counters: *mut ProcessMemoryCounters,
            cb: u32,
        ) -> i32;
    }
    extern "system" {
        fn GetCurrentProcess() -> *mut std::ffi::c_void;
    }

    unsafe {
        let mut pmc: ProcessMemoryCounters = std::mem::zeroed();
        pmc.cb = std::mem::size_of::<ProcessMemoryCounters>() as u32;
        let handle = GetCurrentProcess(); // pseudo-handle, no CloseHandle needed
        if GetProcessMemoryInfo(handle, &mut pmc, pmc.cb) != 0 {
            Some(pmc.working_set_size as u64)
        } else {
            None
        }
    }
}

#[cfg(all(unix, target_os = "linux"))]
pub fn current_rss_bytes() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            let kb_str = rest.trim().trim_end_matches("kB").trim();
            let kb: u64 = kb_str.parse().ok()?;
            return Some(kb * 1024);
        }
    }
    None
}

#[cfg(not(any(windows, all(unix, target_os = "linux"))))]
pub fn current_rss_bytes() -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_rss_bytes_is_a_plausible_process_size_where_supported() {
        // Not asserting an exact value (that would be exactly the kind of
        // hardcoded-baseline test that drifts from reality) -- just that,
        // on a platform this is implemented for, the number is in a sane
        // range for a process that has at least loaded the Rust runtime
        // and this test binary (a few hundred KB at minimum, well under
        // 10GB for a test process).
        if let Some(rss) = current_rss_bytes() {
            assert!(rss > 100_000, "RSS suspiciously small: {rss} bytes");
            assert!(rss < 10_000_000_000, "RSS suspiciously large: {rss} bytes");
        }
    }
}
