//! What machine this ran on.
//!
//! A benchmark number without the machine that produced it is not a
//! result, it is a rumour. "Qu is 3x faster than NumPy" means nothing
//! until it says on what CPU, with how many cores, how much memory, and
//! -- since Qu can dispatch to a GPU -- whether the GPU was used at all.
//! That last one is the easiest to get wrong by accident and the most
//! misleading when it is: a GPU-accelerated run compared against a
//! CPU-only baseline is not a language comparison.
//!
//! **Read, do not guess.** Every field here is either something the
//! operating system actually told us or `None`. Nothing is inferred from
//! something adjacent, and nothing is filled in with a plausible default,
//! because a benchmark report that quietly states the wrong core count is
//! worse than one that says it does not know.
//!
//! No new dependency: this uses `std` plus the per-platform files and
//! environment the OS already publishes. That constrains what can be
//! learned on each platform, which is why several fields are optional --
//! see each one.

use std::fmt::Write as _;

/// The machine, as far as it can be determined here.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SysInfo {
    pub os: String,
    pub arch: String,
    /// Machine name. From `COMPUTERNAME` on Windows, `HOSTNAME` or
    /// `/etc/hostname` elsewhere.
    pub hostname: Option<String>,
    /// CPU model string as the OS reports it.
    pub cpu: Option<String>,
    /// Logical processors -- what `std::thread::available_parallelism`
    /// reports, which respects cgroup and affinity limits rather than
    /// counting the silicon. That is the right number for "how much
    /// parallelism did this run actually have".
    pub logical_cores: Option<usize>,
    /// Physical cores, where the platform exposes them separately.
    pub physical_cores: Option<usize>,
    /// Total system memory in bytes.
    pub total_memory: Option<u64>,
    /// GPU adapter name and backend, when a GPU was successfully
    /// initialised. `None` means no GPU was available *to this build*,
    /// which includes a Qu compiled without the `gpu` feature -- see
    /// `gpu_reason`.
    pub gpu: Option<String>,
    /// Why `gpu` is `None`, when it is.
    pub gpu_reason: Option<String>,
    /// Qu's own version and how it was built. A release-vs-debug mixup is
    /// the single most common way a benchmark comes out an order of
    /// magnitude wrong, so it is reported rather than assumed.
    pub qu_version: String,
    pub build: &'static str,
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

/// Read a `key: value` field out of a `/proc` file.
#[cfg(target_os = "linux")]
fn proc_field(path: &str, key: &str) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    text.lines()
        .find(|l| l.starts_with(key))
        .and_then(|l| l.split_once(':'))
        .map(|(_, v)| v.trim().to_string())
}

#[cfg(target_os = "linux")]
fn platform(info: &mut SysInfo) {
    info.cpu = proc_field("/proc/cpuinfo", "model name");
    // `MemTotal:  16324516 kB` -- the unit is always kB in this file.
    info.total_memory = proc_field("/proc/meminfo", "MemTotal").and_then(|v| {
        v.split_whitespace()
            .next()
            .and_then(|n| n.parse::<u64>().ok())
            .map(|kb| kb * 1024)
    });
    info.hostname = env("HOSTNAME").or_else(|| {
        std::fs::read_to_string("/etc/hostname")
            .ok()
            .map(|h| h.trim().to_string())
            .filter(|h| !h.is_empty())
    });
    // Distinct physical ids x cores per socket. Counting "core id" lines
    // would count each hyperthread.
    if let Ok(text) = std::fs::read_to_string("/proc/cpuinfo") {
        let per_socket: Option<usize> = text
            .lines()
            .find(|l| l.starts_with("cpu cores"))
            .and_then(|l| l.split_once(':'))
            .and_then(|(_, v)| v.trim().parse().ok());
        let sockets = text
            .lines()
            .filter(|l| l.starts_with("physical id"))
            .filter_map(|l| l.split_once(':').map(|(_, v)| v.trim().to_string()))
            .collect::<std::collections::HashSet<_>>()
            .len()
            .max(1);
        info.physical_cores = per_socket.map(|c| c * sockets);
    }
}

#[cfg(target_os = "windows")]
fn platform(info: &mut SysInfo) {
    // Windows publishes the processor identifier and count in the
    // environment of every process, so these need no API call and no
    // dependency. `PROCESSOR_IDENTIFIER` is the family/model/stepping
    // string ("Intel64 Family 6 Model 154 Stepping 3, GenuineIntel"),
    // not the marketing name -- less pretty than "Core i7-12700H" but it
    // is what the OS actually says, and it identifies the part.
    info.cpu = env("PROCESSOR_IDENTIFIER");
    info.hostname = env("COMPUTERNAME");
    // NUMBER_OF_PROCESSORS is logical processors; `available_parallelism`
    // below already covers that and respects affinity, so this is not
    // read as a physical count. Windows does not expose physical cores
    // without an API call, so it stays `None` rather than being guessed
    // at as logical/2 -- that ratio is wrong on any part without
    // hyperthreading, and on efficiency-core designs.
    //
    // Total memory likewise needs `GlobalMemoryStatusEx`, which needs a
    // Windows API dependency this crate does not have. Left `None`.
}

#[cfg(target_os = "macos")]
fn platform(info: &mut SysInfo) {
    info.hostname = env("HOSTNAME");
    // The values here live behind `sysctl`, which needs libc. Left `None`
    // rather than shelling out: a benchmark report that silently depends
    // on spawning a process is a worse trade than an honest gap.
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
fn platform(_info: &mut SysInfo) {}

/// Gather what this machine will tell us.
pub fn collect() -> SysInfo {
    let mut info = SysInfo {
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        logical_cores: std::thread::available_parallelism().ok().map(|n| n.get()),
        qu_version: env!("CARGO_PKG_VERSION").to_string(),
        // `debug_assertions` is on in a debug build and off in a release
        // one -- the honest way to ask "was this optimised", since a
        // benchmark run under `cargo run` without `--release` is commonly
        // 10-50x slower and looks like a language result.
        build: if cfg!(debug_assertions) { "debug" } else { "release" },
        ..Default::default()
    };
    platform(&mut info);
    gpu(&mut info);
    info
}

#[cfg(feature = "gpu")]
fn gpu(info: &mut SysInfo) {
    let probe = crate::gpu_probe::probe();
    match &probe.context {
        Some(ctx) => {
            let a = ctx.adapter_info();
            info.gpu = Some(format!("{} ({:?}, {:?})", a.name, a.backend, a.device_type));
        }
        None => {
            info.gpu_reason = Some(
                probe
                    .init_error
                    .clone()
                    .unwrap_or_else(|| "no GPU adapter was available".to_string()),
            );
        }
    }
}

#[cfg(not(feature = "gpu"))]
fn gpu(info: &mut SysInfo) {
    info.gpu_reason =
        Some("this build has no GPU support (compiled without --features gpu)".to_string());
}

impl SysInfo {
    /// A human-readable block, for the head of a benchmark report.
    pub fn report(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "machine:");
        let _ = writeln!(out, "  os        {} ({})", self.os, self.arch);
        if let Some(h) = &self.hostname {
            let _ = writeln!(out, "  host      {h}");
        }
        if let Some(c) = &self.cpu {
            let _ = writeln!(out, "  cpu       {c}");
        }
        match (self.physical_cores, self.logical_cores) {
            (Some(p), Some(l)) => {
                let _ = writeln!(out, "  cores     {p} physical, {l} logical");
            }
            (None, Some(l)) => {
                let _ = writeln!(out, "  cores     {l} logical");
            }
            _ => {}
        }
        if let Some(m) = self.total_memory {
            let _ = writeln!(out, "  memory    {:.1} GiB", m as f64 / (1024.0 * 1024.0 * 1024.0));
        }
        match (&self.gpu, &self.gpu_reason) {
            (Some(g), _) => {
                let _ = writeln!(out, "  gpu       {g}");
            }
            (None, Some(r)) => {
                let _ = writeln!(out, "  gpu       none -- {r}");
            }
            _ => {}
        }
        let _ = writeln!(out, "  qu        {} ({} build)", self.qu_version, self.build);
        out
    }
}

/// One line naming the machine, for a paper's methods section.
///
/// The convention is a compact clause list, which is what a reviewer
/// expects to see and what makes two papers comparable:
///
/// ```text
/// Intel i9-9880H @ 2.3 GHz, 24 GB RAM, NVMe SSD @ 2500 MB/s;
/// GPU NVIDIA RTX 3500 Ada / 16 GB VRAM
/// ```
///
/// Three of those facts cannot be read on every platform, and two cannot
/// be read anywhere:
///
///   * **Storage throughput** is not a property the OS reports at all --
///     it is a *measurement*, and a different one for sequential and
///     random access. Nothing here can honestly produce it.
///   * **VRAM** and the marketing GPU name come from the adapter, which
///     needs the `gpu` feature.
///   * **Total memory** and the CPU's marketing name are behind Windows
///     API calls this crate does not depend on (see `platform`).
///
/// So `overrides` exists: the author states what the machine cannot, and
/// each stated value REPLACES the detected one rather than being merged
/// with it. That is the honest arrangement -- a methods section is the
/// author's claim about their own hardware, and the alternative is either
/// a blank where a fact belongs or, worse, a plausible guess. Anything not
/// given and not detectable is simply left out of the line rather than
/// printed as "unknown".
pub fn publication_line(info: &SysInfo, overrides: &[(String, String)]) -> String {
    let given = |k: &str| -> Option<&str> {
        overrides
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(k))
            .map(|(_, v)| v.as_str())
    };

    let mut parts: Vec<String> = Vec::new();

    match given("cpu").map(str::to_string).or_else(|| info.cpu.clone()) {
        Some(cpu) => {
            // Cores are appended to the CPU clause rather than given their
            // own, because "16 logical cores" on its own reads as a
            // separate machine property when it is a property of the part
            // just named.
            let cores = match (info.physical_cores, info.logical_cores) {
                (Some(p), Some(l)) => format!(" ({p}C/{l}T)"),
                (None, Some(l)) => format!(" ({l} threads)"),
                _ => String::new(),
            };
            parts.push(format!("{}{}", cpu.trim(), cores));
        }
        None => {}
    }

    if let Some(m) = given("memory").map(str::to_string).or_else(|| {
        info.total_memory
            .map(|b| format!("{:.0} GB RAM", b as f64 / 1e9))
    }) {
        parts.push(m);
    }
    if let Some(d) = given("storage").or_else(|| given("disk")) {
        parts.push(d.to_string());
    }

    let mut line = parts.join(", ");

    // The GPU is a separate clause after a semicolon, because it is a
    // separate machine: whether it was USED is the question a reader has,
    // and burying it in a comma list invites skipping it.
    let gpu = given("gpu").map(str::to_string).or_else(|| info.gpu.clone());
    if let Some(g) = gpu {
        line.push_str(&format!("; GPU {g}"));
    }
    if line.is_empty() {
        line.push_str("machine details unavailable");
    }
    line.push_str(&format!(" [{} {} build, {}]", info.qu_version, info.build, info.os));
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fields that can always be known must always be known. If this
    /// ever reports "unknown" for the OS or the core count, the report is
    /// not worth printing.
    #[test]
    fn always_knows_the_things_std_can_tell_it() {
        let i = collect();
        assert!(!i.os.is_empty());
        assert!(!i.arch.is_empty());
        assert!(i.logical_cores.unwrap_or(0) >= 1, "at least one core must be reported");
        assert!(!i.qu_version.is_empty());
        assert!(i.build == "debug" || i.build == "release");
    }

    /// A test binary is built with `debug_assertions` off under
    /// `--release`, which is the distinction the field exists to make.
    #[test]
    fn reports_the_build_profile_it_was_actually_compiled_with() {
        let i = collect();
        let expected = if cfg!(debug_assertions) { "debug" } else { "release" };
        assert_eq!(i.build, expected);
    }

    /// Either a GPU is named or there is a reason it is not -- never both
    /// empty, which would read as "no GPU section" and leave a reader
    /// unable to tell a CPU-only run from an unreported one.
    #[test]
    fn a_missing_gpu_always_comes_with_a_reason() {
        let i = collect();
        assert!(
            i.gpu.is_some() || i.gpu_reason.is_some(),
            "a run with no GPU must say why, or the report cannot be read"
        );
    }

    /// An author-stated fact replaces the detected one. A methods section
    /// is the author's claim about their own hardware, and there is no
    /// case where a half-detected string should win over what they wrote.
    #[test]
    fn stated_hardware_replaces_detected_hardware() {
        let mut info = collect();
        info.cpu = Some("Detected Nonsense CPU".into());
        let line = publication_line(
            &info,
            &[("cpu".into(), "Intel i9-9880H @ 2.3 GHz".into())],
        );
        assert!(line.contains("Intel i9-9880H"), "{line}");
        assert!(!line.contains("Nonsense"), "{line}");
    }

    /// A fact nobody can determine -- disk throughput is a measurement,
    /// not a property -- is simply absent unless stated, never guessed and
    /// never printed as "unknown".
    #[test]
    fn undetectable_facts_are_omitted_rather_than_invented() {
        let info = SysInfo { qu_version: "0.1.0".into(), build: "release", ..Default::default() };
        let bare = publication_line(&info, &[]);
        assert!(!bare.to_lowercase().contains("unknown"), "{bare}");
        assert!(!bare.contains("MB/s"), "{bare}");
        let stated = publication_line(&info, &[("storage".into(), "NVMe SSD @ 2500 MB/s".into())]);
        assert!(stated.contains("2500 MB/s"), "{stated}");
    }

    /// The GPU goes after a semicolon, on its own. Whether one was
    /// involved is the first thing a reader checks about a benchmark, and
    /// a comma list invites skipping it.
    #[test]
    fn the_gpu_is_its_own_clause() {
        let info = SysInfo { qu_version: "0.1.0".into(), build: "release", ..Default::default() };
        let line = publication_line(&info, &[("gpu".into(), "NVIDIA RTX 3500 Ada / 16 GB VRAM".into())]);
        assert!(line.contains("; GPU NVIDIA RTX 3500 Ada"), "{line}");
    }

    /// The build profile always survives into the line. A benchmark run
    /// unoptimised is commonly 10-50x slow and reads as a language
    /// result, so it is the one thing that must never be droppable.
    #[test]
    fn the_build_profile_is_always_stated() {
        let info = SysInfo { qu_version: "0.1.0".into(), build: "debug", ..Default::default() };
        assert!(publication_line(&info, &[]).contains("debug build"));
    }

    #[test]
    fn the_report_names_the_machine_and_the_build() {
        let text = collect().report();
        assert!(text.contains("os "), "{text}");
        assert!(text.contains("qu "), "{text}");
        assert!(text.contains("gpu"), "{text}");
    }
}
