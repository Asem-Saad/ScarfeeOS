//! Hardware detection and AI tier recommendation for the installer.
//! Parsing is done by pure functions over text so it can be tested with fixtures.

use std::fs;
use std::path::Path;
use std::process::Command;

// Thresholds are a project owner decision, set from benchmark measurements (see docs/models.md).
/// Some cards report slightly less memory than their nominal size.
const GPU_MEMORY_TOLERANCE_MIB: u64 = 128;
/// Smallest GPU memory, in MiB, for which the Full tier is recommended:
/// 6 GB minus the tolerance.
pub const FULL_MIN_GPU_MEMORY_MIB: u64 = 6 * 1024 - GPU_MEMORY_TOLERANCE_MIB;
/// Tolerance for the 8 GB and 12 GB boundaries. Smaller than the one above so that
/// a card with 11.9 GB (12186 MiB) still counts as below 12 GB.
pub const SMALL_GPU_MEMORY_TOLERANCE_MIB: u64 = 64;
/// Smallest GPU memory, in MiB, for which the Dual tier is recommended (two models loaded):
/// 8 GB minus the small tolerance. An RTX 4060 8 GB reports 8188 MiB.
pub const DUAL_MIN_GPU_MEMORY_MIB: u64 = 8 * 1024 - SMALL_GPU_MEMORY_TOLERANCE_MIB;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// No suitable GPU: one small model on the CPU.
    Lite,
    /// One larger model on the GPU.
    Full,
    /// Two models on the GPU: commands and Arabic writing.
    Dual,
}

/// An AMD GPU whose dedicated memory (`mem_info_vram_total`) is below this many MiB is treated
/// as integrated: the firmware only reserves a small slice of system memory for it.
const AMD_INTEGRATED_MAX_VRAM_MIB: u64 = 1024;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuVendor {
    Amd,
    Intel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuKind {
    /// Has its own memory.
    Discrete,
    /// Shares system memory.
    Integrated,
    /// The files that tell the two apart could not be read.
    Unknown,
}

/// An AMD or Intel GPU found under `/sys/class/drm`. NVIDIA GPUs come from `nvidia-smi` instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuDevice {
    pub vendor: GpuVendor,
    /// sysfs has no product name, so this is the vendor and the PCI id, such as `AMD GPU 1002:73FF`.
    pub name: String,
    pub kind: GpuKind,
    /// Dedicated memory in MiB. None for integrated GPUs, which share system memory, and when it
    /// cannot be read: sysfs has no memory size for Intel GPUs, so it is always None for them.
    pub dedicated_memory_mib: Option<u64>,
    /// True when llama.cpp has a documented backend for this vendor's GPUs and the kernel driver
    /// that backend needs is bound to it. It does not check that the llama.cpp build in use
    /// includes that backend: ScarfeeOS has no packaged build yet.
    pub llama_cpp_usable: bool,
}

impl GpuDevice {
    /// The GPU as the tier advice sees it: only a GPU that llama.cpp can use and that has a
    /// known amount of dedicated memory.
    pub fn as_gpu(&self) -> Option<Gpu> {
        if !self.llama_cpp_usable {
            return None;
        }
        Some(Gpu { name: self.name.clone(), memory_mib: self.dedicated_memory_mib? })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hardware {
    /// None when the total could not be read.
    pub ram_kib: Option<u64>,
    pub cpu: CpuFlags,
    /// The usable GPU with the most dedicated memory; None when there is none. A GPU that llama.cpp
    /// cannot use, an integrated GPU and a GPU of unknown memory are never listed here (see
    /// `detect_gpu_devices` for those).
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

/// Value of `KEY=` in sysfs `uevent` text.
fn uevent_value<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines().find_map(|l| l.strip_prefix(key)?.strip_prefix('='))
}

/// Reads one `/sys/class/drm/cardN` folder. Anything that is not an AMD or Intel PCI GPU gives None.
fn read_gpu_device(card: &Path) -> Option<GpuDevice> {
    let dev = card.join("device");
    let read = |file: &str| fs::read_to_string(dev.join(file)).ok();
    let vendor = match read("vendor")?.trim() {
        "0x1002" => GpuVendor::Amd,
        "0x8086" => GpuVendor::Intel,
        _ => return None,
    };
    let uevent = read("uevent").unwrap_or_default();
    let driver = uevent_value(&uevent, "DRIVER");
    let vendor_name = match vendor {
        GpuVendor::Amd => "AMD",
        GpuVendor::Intel => "Intel",
    };
    let name = match uevent_value(&uevent, "PCI_ID") {
        Some(id) => format!("{vendor_name} GPU {id}"),
        None => format!("{vendor_name} GPU"),
    };
    let (kind, dedicated_memory_mib) = match vendor {
        GpuVendor::Amd => {
            let vram_mib = read("mem_info_vram_total")
                .and_then(|t| t.trim().parse::<u64>().ok())
                .map(|bytes| bytes / (1024 * 1024));
            match vram_mib {
                Some(m) if m >= AMD_INTEGRATED_MAX_VRAM_MIB => (GpuKind::Discrete, Some(m)),
                Some(_) => (GpuKind::Integrated, None),
                None => (GpuKind::Unknown, None),
            }
        }
        // Intel integrated GPUs sit on PCI bus 00 (0000:00:02.0); discrete ones do not.
        GpuVendor::Intel => {
            let bus = uevent_value(&uevent, "PCI_SLOT_NAME").and_then(|slot| slot.split(':').nth(1));
            match bus {
                Some("00") => (GpuKind::Integrated, None),
                Some(_) => (GpuKind::Discrete, None),
                None => (GpuKind::Unknown, None),
            }
        }
    };
    // llama.cpp's HIP (AMD) and Vulkan backends need a working AMD driver stack, and its SYCL and
    // Vulkan backends an Intel one. AMD's Vulkan and ROCm stacks run on the amdgpu kernel driver
    // (not stated in llama.cpp's docs); Intel's on i915 or xe.
    let llama_cpp_usable = matches!(
        (vendor, driver),
        (GpuVendor::Amd, Some("amdgpu")) | (GpuVendor::Intel, Some("i915" | "xe"))
    );
    Some(GpuDevice { vendor, name, kind, dedicated_memory_mib, llama_cpp_usable })
}

/// Finds AMD and Intel GPUs under `<root>/sys/class/drm`. `root` is `/` on a real machine and a
/// fake folder in tests. A missing folder gives no GPUs.
pub fn detect_gpu_devices_in(root: &Path) -> Vec<GpuDevice> {
    let Ok(entries) = fs::read_dir(root.join("sys/class/drm")) else { return Vec::new() };
    let mut cards: Vec<_> = entries
        .filter_map(Result::ok)
        .filter(|e| {
            let name = e.file_name();
            // Skips `card0-eDP-1` connectors, `renderD128` and `version`.
            name.to_str()
                .and_then(|n| n.strip_prefix("card"))
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        })
        .map(|e| e.path())
        .collect();
    cards.sort();
    cards.iter().filter_map(|c| read_gpu_device(c)).collect()
}

/// Finds AMD and Intel GPUs on this machine. Always empty off Linux.
pub fn detect_gpu_devices() -> Vec<GpuDevice> {
    detect_gpu_devices_in(Path::new("/"))
}

/// The GPU the tier advice uses: the one with the most dedicated memory among the NVIDIA GPU and
/// the usable AMD and Intel ones. NVIDIA wins a tie: the memory figures in `tiers.rs` were
/// measured with CUDA.
pub fn best_gpu(nvidia: Option<Gpu>, devices: &[GpuDevice]) -> Option<Gpu> {
    devices
        .iter()
        .filter_map(GpuDevice::as_gpu)
        .chain(nvidia)
        .max_by_key(|g| g.memory_mib)
}

/// Detects the hardware of this machine. Anything unreadable is left empty.
pub fn detect() -> Hardware {
    let read = |path| fs::read_to_string(path).unwrap_or_default();
    Hardware {
        ram_kib: parse_meminfo(&read("/proc/meminfo")),
        cpu: parse_cpu_flags(&read("/proc/cpuinfo")),
        gpu: best_gpu(detect_gpu(), &detect_gpu_devices()),
    }
}

/// Recommends a tier. This is advice only: the user may choose any tier.
pub fn recommend_tier(hw: &Hardware) -> Recommendation {
    let (tier, reason) = match &hw.gpu {
        Some(gpu) if gpu.memory_mib >= DUAL_MIN_GPU_MEMORY_MIB => (
            Tier::Dual,
            format!(
                "{} has {} MiB of memory (at least {} MiB needed)",
                gpu.name, gpu.memory_mib, DUAL_MIN_GPU_MEMORY_MIB
            ),
        ),
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
        None => (Tier::Lite, "no GPU with usable dedicated memory detected".to_string()),
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
        let rec = recommend_tier(&hw_with(Some(gpu("RTX 3050", 6144))));
        assert_eq!(rec.tier, Tier::Full);
        assert!(rec.reason.contains("RTX 3050"));
    }

    #[test]
    fn gpu_with_much_memory_gets_dual() {
        let rec = recommend_tier(&hw_with(Some(gpu("RTX 3060", 12288))));
        assert_eq!(rec.tier, Tier::Dual);
        assert!(rec.reason.contains("RTX 3060"));
    }

    #[test]
    fn thresholds_are_inclusive() {
        let tier_at = |mib| recommend_tier(&hw_with(Some(gpu("A", mib)))).tier;
        assert_eq!(tier_at(FULL_MIN_GPU_MEMORY_MIB), Tier::Full);
        assert_eq!(tier_at(FULL_MIN_GPU_MEMORY_MIB - 1), Tier::Lite);
        assert_eq!(tier_at(DUAL_MIN_GPU_MEMORY_MIB), Tier::Dual);
        assert_eq!(tier_at(DUAL_MIN_GPU_MEMORY_MIB - 1), Tier::Full);
    }

    #[test]
    fn card_reporting_8188_mib_gets_dual() {
        assert_eq!(DUAL_MIN_GPU_MEMORY_MIB, 8128);
        let rec = recommend_tier(&hw_with(Some(gpu("RTX 4060", 8188))));
        assert_eq!(rec.tier, Tier::Dual);
        assert_eq!(recommend_tier(&hw_with(Some(gpu("A", 8127)))).tier, Tier::Full);
    }

    #[test]
    fn card_reporting_slightly_under_6_gb_gets_full() {
        let rec = recommend_tier(&hw_with(Some(gpu("A", 6144 - 100))));
        assert_eq!(rec.tier, Tier::Full);
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
        assert!(rec.reason.contains("no GPU with usable dedicated memory"));
    }

    /// A fake root folder with a `sys/class/drm` tree, removed on drop.
    struct FakeRoot(std::path::PathBuf);

    impl FakeRoot {
        fn new() -> FakeRoot {
            use std::sync::atomic::{AtomicU32, Ordering};
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let dir = std::env::temp_dir().join(format!(
                "scarfeeos-hw-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(dir.join("sys/class/drm")).unwrap();
            FakeRoot(dir)
        }

        fn write(&self, rel: &str, text: &str) {
            let path = self.0.join("sys/class/drm").join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }

        /// Adds `cardN` with a PCI device description.
        fn card(&self, n: u32, vendor: &str, driver: &str, pci_id: &str, slot: &str) {
            let uevent = format!("DRIVER={driver}\nPCI_ID={pci_id}\nPCI_SLOT_NAME={slot}\n");
            self.write(&format!("card{n}/device/vendor"), &format!("{vendor}\n"));
            self.write(&format!("card{n}/device/uevent"), &uevent);
        }

        fn vram(&self, n: u32, bytes: u64) {
            self.write(&format!("card{n}/device/mem_info_vram_total"), &format!("{bytes}\n"));
        }

        fn devices(&self) -> Vec<GpuDevice> {
            detect_gpu_devices_in(&self.0)
        }
    }

    impl Drop for FakeRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    const MIB: u64 = 1024 * 1024;

    #[test]
    fn amd_discrete_gpu_reports_vram_total() {
        let root = FakeRoot::new();
        root.card(1, "0x1002", "amdgpu", "1002:73FF", "0000:03:00.0");
        root.vram(1, 8176 * MIB);
        assert_eq!(
            root.devices(),
            vec![GpuDevice {
                vendor: GpuVendor::Amd,
                name: "AMD GPU 1002:73FF".to_string(),
                kind: GpuKind::Discrete,
                dedicated_memory_mib: Some(8176),
                llama_cpp_usable: true,
            }]
        );
        assert_eq!(best_gpu(None, &root.devices()), Some(gpu("AMD GPU 1002:73FF", 8176)));
    }

    #[test]
    fn amd_apu_with_a_small_carve_out_is_integrated_and_has_no_dedicated_memory() {
        let root = FakeRoot::new();
        root.card(0, "0x1002", "amdgpu", "1002:1638", "0000:05:00.0");
        root.vram(0, 512 * MIB);
        let devices = root.devices();
        assert_eq!(devices[0].kind, GpuKind::Integrated);
        assert_eq!(devices[0].dedicated_memory_mib, None);
        assert!(devices[0].llama_cpp_usable);
        assert_eq!(best_gpu(None, &devices), None);
        // The boundary: 1 GiB counts as dedicated, 1 GiB minus a byte does not.
        root.vram(0, 1024 * MIB);
        assert_eq!(root.devices()[0].dedicated_memory_mib, Some(1024));
        root.vram(0, 1024 * MIB - 1);
        assert_eq!(root.devices()[0].kind, GpuKind::Integrated);
    }

    #[test]
    fn amd_gpu_without_vram_file_has_unknown_kind_and_memory() {
        let root = FakeRoot::new();
        root.card(0, "0x1002", "amdgpu", "1002:73FF", "0000:03:00.0");
        let d = &root.devices()[0];
        assert_eq!((d.kind, d.dedicated_memory_mib), (GpuKind::Unknown, None));
        root.write("card0/device/mem_info_vram_total", "not a number\n");
        assert_eq!(root.devices()[0].dedicated_memory_mib, None);
        assert_eq!(best_gpu(None, &root.devices()), None);
    }

    #[test]
    fn amd_gpu_on_the_old_radeon_driver_is_not_usable() {
        let root = FakeRoot::new();
        root.card(0, "0x1002", "radeon", "1002:6798", "0000:01:00.0");
        root.vram(0, 8192 * MIB);
        let d = &root.devices()[0];
        assert_eq!((d.kind, d.dedicated_memory_mib, d.llama_cpp_usable), (GpuKind::Discrete, Some(8192), false));
        assert_eq!(best_gpu(None, &root.devices()), None);
    }

    #[test]
    fn gpu_with_no_driver_bound_is_not_usable() {
        let root = FakeRoot::new();
        root.write("card0/device/vendor", "0x1002\n");
        root.write("card0/device/uevent", "PCI_ID=1002:73FF\n");
        root.vram(0, 8192 * MIB);
        assert!(!root.devices()[0].llama_cpp_usable);
        root.card(1, "0x1002", "vfio-pci", "1002:73FF", "0000:03:00.0");
        assert!(!root.devices()[1].llama_cpp_usable);
    }

    #[test]
    fn intel_integrated_gpu_is_on_pci_bus_zero_and_has_no_dedicated_memory() {
        let root = FakeRoot::new();
        for (n, driver) in [(0, "i915"), (1, "xe")] {
            root.card(n, "0x8086", driver, "8086:46A6", "0000:00:02.0");
        }
        let devices = root.devices();
        for d in &devices {
            assert_eq!(d.vendor, GpuVendor::Intel);
            assert_eq!(d.name, "Intel GPU 8086:46A6");
            assert_eq!((d.kind, d.dedicated_memory_mib, d.llama_cpp_usable), (GpuKind::Integrated, None, true));
        }
        assert_eq!(best_gpu(None, &devices), None);
    }

    #[test]
    fn intel_discrete_gpu_has_unknown_memory_so_it_cannot_raise_the_tier() {
        let root = FakeRoot::new();
        root.card(0, "0x8086", "xe", "8086:56A0", "0000:03:00.0");
        let d = &root.devices()[0];
        assert_eq!((d.kind, d.dedicated_memory_mib, d.llama_cpp_usable), (GpuKind::Discrete, None, true));
        assert_eq!(best_gpu(None, &root.devices()), None);
        assert_eq!(recommend_tier(&hw_with(best_gpu(None, &root.devices()))).tier, Tier::Lite);
    }

    #[test]
    fn intel_gpu_without_a_pci_slot_has_unknown_kind() {
        let root = FakeRoot::new();
        root.write("card0/device/vendor", "0x8086\n");
        root.write("card0/device/uevent", "DRIVER=i915\n");
        let d = &root.devices()[0];
        assert_eq!((d.kind, d.name.as_str()), (GpuKind::Unknown, "Intel GPU"));
    }

    #[test]
    fn only_card_folders_with_amd_or_intel_vendors_are_read() {
        let root = FakeRoot::new();
        root.card(0, "0x10de", "nvidia", "10DE:2504", "0000:01:00.0");
        root.write("card0-eDP-1/status", "connected\n");
        root.write("card0-eDP-1/device/vendor", "0x1002\n");
        root.write("renderD128/device/vendor", "0x1002\n");
        root.write("version", "drm 1.1.0\n");
        root.write("cardx/device/vendor", "0x1002\n");
        root.write("card/device/vendor", "0x1002\n");
        root.write("card3/device/uevent", "DRIVER=amdgpu\n");
        assert_eq!(root.devices(), Vec::new());
    }

    #[test]
    fn missing_sys_class_drm_gives_no_gpus() {
        let root = FakeRoot::new();
        fs::remove_dir_all(root.0.join("sys")).unwrap();
        assert_eq!(root.devices(), Vec::new());
    }

    #[test]
    fn cards_are_read_in_name_order() {
        let root = FakeRoot::new();
        root.card(1, "0x1002", "amdgpu", "1002:73FF", "0000:03:00.0");
        root.vram(1, 8176 * MIB);
        root.card(0, "0x8086", "i915", "8086:46A6", "0000:00:02.0");
        let vendors: Vec<_> = root.devices().iter().map(|d| d.vendor).collect();
        assert_eq!(vendors, [GpuVendor::Intel, GpuVendor::Amd]);
    }

    #[test]
    fn hybrid_laptop_uses_the_discrete_amd_gpu_not_the_integrated_one() {
        let root = FakeRoot::new();
        root.card(0, "0x8086", "i915", "8086:46A6", "0000:00:02.0");
        root.card(1, "0x1002", "amdgpu", "1002:73FF", "0000:03:00.0");
        root.vram(1, 8176 * MIB);
        let best = best_gpu(None, &root.devices());
        assert_eq!(best, Some(gpu("AMD GPU 1002:73FF", 8176)));
        assert_eq!(recommend_tier(&hw_with(best)).tier, Tier::Dual);
    }

    #[test]
    fn amd_apu_laptop_stays_lite_however_much_system_memory_it_has() {
        let root = FakeRoot::new();
        root.card(0, "0x1002", "amdgpu", "1002:1638", "0000:05:00.0");
        root.vram(0, 512 * MIB);
        let hw = Hardware { ram_kib: Some(64 * 1024 * 1024), gpu: best_gpu(None, &root.devices()), ..Hardware::default() };
        assert_eq!(recommend_tier(&hw).tier, Tier::Lite);
    }

    #[test]
    fn a_big_gpu_with_no_usable_backend_does_not_raise_the_tier() {
        let root = FakeRoot::new();
        root.card(0, "0x1002", "radeon", "1002:6798", "0000:01:00.0");
        root.vram(0, 16 * 1024 * MIB);
        let hw = hw_with(best_gpu(None, &root.devices()));
        assert_eq!(recommend_tier(&hw).tier, Tier::Lite);
    }

    #[test]
    fn nvidia_wins_a_tie_and_a_bigger_amd_gpu_wins_over_nvidia() {
        let amd = |mib| GpuDevice {
            vendor: GpuVendor::Amd,
            name: "AMD".to_string(),
            kind: GpuKind::Discrete,
            dedicated_memory_mib: Some(mib),
            llama_cpp_usable: true,
        };
        assert_eq!(best_gpu(Some(gpu("RTX", 8188)), &[amd(8188)]), Some(gpu("RTX", 8188)));
        assert_eq!(best_gpu(Some(gpu("RTX", 6144)), &[amd(16368)]), Some(gpu("AMD", 16368)));
        assert_eq!(best_gpu(Some(gpu("RTX", 6144)), &[]), Some(gpu("RTX", 6144)));
        assert_eq!(best_gpu(None, &[]), None);
    }

    #[test]
    fn detect_gpu_devices_does_not_panic() {
        let _ = detect_gpu_devices();
    }

    #[test]
    fn detect_does_not_panic_without_nvidia_smi() {
        let _ = detect();
    }
}
