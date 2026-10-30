//! Hardware detection and AI tier recommendation for the installer.
//! Parsing is done by pure functions over text so it can be tested with fixtures.

use std::fs;
use std::process::Command;

// Provisional thresholds: benchmarks will set the final values (see docs/models.md).
/// Smallest GPU memory, in MiB, for which the Full tier is recommended.
pub const FULL_MIN_GPU_MEMORY_MIB: u64 = 6 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Full,
    Lite,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CpuFlags {
    pub avx2: bool,
    pub avx512f: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gpu {
    pub name: String,
    pub memory_mib: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hardware {
    /// None when the total could not be read.
    pub ram_kib: Option<u64>,
    pub cpu: CpuFlags,
    /// None when no NVIDIA GPU was detected.
    pub gpu: Option<Gpu>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recommendation {
    pub tier: Tier,
    pub reason: String,
}

/// Reads `MemTotal` from `/proc/meminfo` text, in KiB.
pub fn parse_meminfo(text: &str) -> Option<u64> {
    let rest = text.lines().find_map(|l| l.strip_prefix("MemTotal:"))?;
    let mut parts = rest.split_whitespace();
    let value: u64 = parts.next()?.parse().ok()?;
    match parts.next() {
        Some("kB") => Some(value),
        _ => None,
    }
}

/// Reads the AVX flags from the first `flags` line of `/proc/cpuinfo` text.
/// Text without such a line gives no flags.
pub fn parse_cpu_flags(text: &str) -> CpuFlags {
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else { continue };
        if key.trim() != "flags" {
            continue;
        }
        let has = |flag: &str| value.split_whitespace().any(|f| f == flag);
        return CpuFlags { avx2: has("avx2"), avx512f: has("avx512f") };
    }
    CpuFlags::default()
}

/// Parses `nvidia-smi --query-gpu=name,memory.total --format=csv,noheader`
/// output such as `NVIDIA GeForce RTX 3060, 12288 MiB`. Malformed lines are
/// skipped. With several GPUs, the one with the most memory is returned.
pub fn parse_nvidia_smi(text: &str) -> Option<Gpu> {
    text.lines()
        .filter_map(|line| {
            let (name, mem) = line.rsplit_once(',')?;
            let name = name.trim();
            let memory_mib: u64 = mem.trim().strip_suffix("MiB")?.trim().parse().ok()?;
            if name.is_empty() {
                return None;
            }
            Some(Gpu { name: name.to_string(), memory_mib })
        })
        .max_by_key(|g| g.memory_mib)
}

/// Runs `nvidia-smi`. A missing command, or one that fails, means no NVIDIA GPU.
fn detect_gpu() -> Option<Gpu> {
    let output = Command::new("nvidia-smi")
        .args(["--query-gpu=name,memory.total", "--format=csv,noheader"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_nvidia_smi(&String::from_utf8_lossy(&output.stdout))
}

/// Detects the hardware of this machine. Anything unreadable is left empty.
pub fn detect() -> Hardware {
    let read = |path| fs::read_to_string(path).unwrap_or_default();
    Hardware {
        ram_kib: parse_meminfo(&read("/proc/meminfo")),
        cpu: parse_cpu_flags(&read("/proc/cpuinfo")),
        gpu: detect_gpu(),
    }
}

/// Recommends a tier. This is advice only: the user may choose either tier.
pub fn recommend_tier(hw: &Hardware) -> Recommendation {
    let (tier, reason) = match &hw.gpu {
        Some(gpu) if gpu.memory_mib >= FULL_MIN_GPU_MEMORY_MIB => (
            Tier::Full,
            format!(
                "{} has {} MiB of memory (at least {} MiB needed)",
                gpu.name, gpu.memory_mib, FULL_MIN_GPU_MEMORY_MIB
            ),
        ),
        Some(gpu) => (
            Tier::Lite,
            format!(
                "{} has {} MiB of memory (at least {} MiB needed for Full)",
                gpu.name, gpu.memory_mib, FULL_MIN_GPU_MEMORY_MIB
            ),
        ),
        None => (Tier::Lite, "no NVIDIA GPU detected".to_string()),
    };
    Recommendation { tier, reason }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MEMINFO: &str = "MemTotal:       16384000 kB\nMemFree:         1234567 kB\n";
    const CPUINFO: &str = "processor\t: 0\nmodel name\t: Test CPU\nflags\t\t: fpu sse2 avx avx2 avx512f avx512bw\n\nprocessor\t: 1\nflags\t\t: fpu sse2 avx avx2 avx512f avx512bw\n";

    fn gpu(name: &str, memory_mib: u64) -> Gpu {
        Gpu { name: name.to_string(), memory_mib }
    }

    fn hw_with(gpu: Option<Gpu>) -> Hardware {
        Hardware { gpu, ..Hardware::default() }
    }

    #[test]
    fn parses_meminfo() {
        assert_eq!(parse_meminfo(MEMINFO), Some(16384000));
    }

    #[test]
    fn bad_meminfo_gives_none() {
        assert_eq!(parse_meminfo(""), None);
        assert_eq!(parse_meminfo("MemFree: 100 kB\n"), None);
        assert_eq!(parse_meminfo("MemTotal: lots kB\n"), None);
        assert_eq!(parse_meminfo("MemTotal: 100\n"), None);
        assert_eq!(parse_meminfo("MemTotal: 100 MB\n"), None);
        assert_eq!(parse_meminfo("MemTotal:\n"), None);
    }

    #[test]
    fn parses_cpu_flags() {
        assert_eq!(parse_cpu_flags(CPUINFO), CpuFlags { avx2: true, avx512f: true });
        let avx2_only = "flags : fpu avx avx2 avx512bw\n";
        assert_eq!(parse_cpu_flags(avx2_only), CpuFlags { avx2: true, avx512f: false });
    }

    #[test]
    fn cpu_flags_match_whole_words_only() {
        let text = "flags : fpu avx2x notavx512f\n";
        assert_eq!(parse_cpu_flags(text), CpuFlags::default());
    }

    #[test]
    fn missing_or_malformed_cpuinfo_gives_no_flags() {
        assert_eq!(parse_cpu_flags(""), CpuFlags::default());
        assert_eq!(parse_cpu_flags("flags avx2 avx512f\n"), CpuFlags::default());
        assert_eq!(parse_cpu_flags("model name : avx2\n"), CpuFlags::default());
        // ARM machines list "Features", not "flags".
        assert_eq!(parse_cpu_flags("Features : fp asimd\n"), CpuFlags::default());
    }

    #[test]
    fn parses_nvidia_smi() {
        assert_eq!(
            parse_nvidia_smi("NVIDIA GeForce RTX 3060, 12288 MiB\n"),
            Some(gpu("NVIDIA GeForce RTX 3060", 12288))
        );
    }

    #[test]
    fn nvidia_smi_picks_largest_gpu() {
        let text = "NVIDIA GeForce GTX 1050, 2048 MiB\nNVIDIA RTX A4000, 16376 MiB\n";
        assert_eq!(parse_nvidia_smi(text), Some(gpu("NVIDIA RTX A4000", 16376)));
    }

    #[test]
    fn nvidia_smi_name_may_contain_a_comma() {
        assert_eq!(parse_nvidia_smi("Foo, Bar 9, 8192 MiB\n"), Some(gpu("Foo, Bar 9", 8192)));
    }

    #[test]
    fn bad_nvidia_smi_output_gives_none() {
        assert_eq!(parse_nvidia_smi(""), None);
        assert_eq!(parse_nvidia_smi("\n\n"), None);
        assert_eq!(parse_nvidia_smi("no name or memory"), None);
        assert_eq!(parse_nvidia_smi("RTX 3060, lots MiB"), None);
        assert_eq!(parse_nvidia_smi("RTX 3060, 12288"), None);
        assert_eq!(parse_nvidia_smi(", 12288 MiB"), None);
        assert_eq!(
            parse_nvidia_smi("NVIDIA-SMI has failed because it couldn't communicate with the NVIDIA driver.\n"),
            None
        );
    }

    #[test]
    fn nvidia_smi_skips_bad_lines_but_keeps_good_ones() {
        let text = "garbage\nRTX 3060, 12288 MiB\n";
        assert_eq!(parse_nvidia_smi(text), Some(gpu("RTX 3060", 12288)));
    }

    #[test]
    fn gpu_with_enough_memory_gets_full() {
        let rec = recommend_tier(&hw_with(Some(gpu("RTX 3060", 12288))));
        assert_eq!(rec.tier, Tier::Full);
        assert!(rec.reason.contains("RTX 3060"));
    }

    #[test]
    fn threshold_is_inclusive() {
        let at = recommend_tier(&hw_with(Some(gpu("A", FULL_MIN_GPU_MEMORY_MIB))));
        let below = recommend_tier(&hw_with(Some(gpu("A", FULL_MIN_GPU_MEMORY_MIB - 1))));
        assert_eq!(at.tier, Tier::Full);
        assert_eq!(below.tier, Tier::Lite);
    }

    #[test]
    fn small_gpu_gets_lite() {
        let rec = recommend_tier(&hw_with(Some(gpu("GTX 1050", 2048))));
        assert_eq!(rec.tier, Tier::Lite);
        assert!(rec.reason.contains("2048"));
    }

    #[test]
    fn no_gpu_gets_lite() {
        let rec = recommend_tier(&Hardware::default());
        assert_eq!(rec.tier, Tier::Lite);
        assert!(rec.reason.contains("no NVIDIA GPU"));
    }

    #[test]
    fn detect_does_not_panic_without_nvidia_smi() {
        let _ = detect();
    }
}
