//! Model plan for each AI tier. This is advice only: nothing here enforces it,
//! and the user may override any recommendation.
//! Memory figures were measured on an RTX 3050 with 6 GB, llama.cpp, Q4_K_M files.

use crate::hardware::Tier;

/// Smallest GPU memory, in MiB, for which Dual uses Qwen3-8B with Jais:
/// 12 GB (the pair needs about 9.8 GB measured, plus headroom).
/// Below it, Dual uses Qwen3-4B with Jais (about 7.7 GB measured).
pub const DUAL_LARGE_MIN_GPU_MEMORY_MIB: u64 = 12 * 1024;

/// llama-server `--n-gpu-layers` value that offloads every layer to the GPU.
pub const ALL_GPU_LAYERS: u32 = 999;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelSpec {
    pub name: &'static str,
    /// Hugging Face repository.
    pub repo: &'static str,
    /// File name inside the repository.
    pub file: &'static str,
    /// Memory in MiB. The measured figures were given in GB and are taken as GiB.
    pub memory_mib: u64,
    /// True when `memory_mib` was measured, false when it is an estimate.
    pub measured: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelPlan {
    pub tier: Tier,
    /// Handles commands (and chat, in Lite).
    pub command: ModelSpec,
    /// Handles Arabic writing and explanation; None when one model does everything.
    pub writer: Option<ModelSpec>,
    /// llama-server `--n-gpu-layers`: 0 keeps the model on the CPU.
    pub gpu_layers: u32,
}

// Measured GPU memory: 3.7 GB.
pub const QWEN3_4B: ModelSpec = ModelSpec {
    name: "Qwen3-4B",
    repo: "Qwen/Qwen3-4B-GGUF",
    file: "Qwen3-4B-Q4_K_M.gguf",
    memory_mib: 3789,
    measured: true,
};

// Measured GPU memory: 5.8 GB.
pub const QWEN3_8B: ModelSpec = ModelSpec {
    name: "Qwen3-8B",
    repo: "Qwen/Qwen3-8B-GGUF",
    file: "Qwen3-8B-Q4_K_M.gguf",
    memory_mib: 5939,
    measured: true,
};

// Measured GPU memory: 4.0 GB.
pub const JAIS_3_3_7B: ModelSpec = ModelSpec {
    name: "Jais-3-3.7B",
    repo: "IFM/Jais-3-3.7B-GGUF",
    file: "Jais-3-3.7B-Q4_K_M.gguf",
    memory_mib: 4096,
    measured: true,
};

/// Model for the Lite tier. NOT FINAL: the CPU benchmark has not finished.
/// Its memory is an estimate: the 3.7 GB was measured on a GPU, not as CPU RAM use.
pub const LITE_MODEL: ModelSpec = ModelSpec { measured: false, ..QWEN3_4B };

/// Plans the models for `tier`. `gpu_memory_mib` is None when there is no GPU.
/// Only Dual looks at it, to pick the pair. A tier that does not fit the
/// hardware still gets its plan.
pub fn plan_for(tier: Tier, gpu_memory_mib: Option<u64>) -> ModelPlan {
    match tier {
        Tier::Lite => ModelPlan { tier, command: LITE_MODEL, writer: None, gpu_layers: 0 },
        Tier::Full => {
            ModelPlan { tier, command: QWEN3_8B, writer: None, gpu_layers: ALL_GPU_LAYERS }
        }
        Tier::Dual => {
            let large = gpu_memory_mib.is_some_and(|m| m >= DUAL_LARGE_MIN_GPU_MEMORY_MIB);
            let command = if large { QWEN3_8B } else { QWEN3_4B };
            ModelPlan { tier, command, writer: Some(JAIS_3_3_7B), gpu_layers: ALL_GPU_LAYERS }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::{recommend_tier, Gpu, Hardware};

    const GB: u64 = 1024;

    /// Recommended tier for a GPU with this much memory, then its plan.
    fn plan_for_gpu(mib: Option<u64>) -> ModelPlan {
        let gpu = mib.map(|memory_mib| Gpu { name: "Test GPU".to_string(), memory_mib });
        let tier = recommend_tier(&Hardware { gpu, ..Hardware::default() }).tier;
        plan_for(tier, mib)
    }

    fn assert_plan(plan: ModelPlan, tier: Tier, command: ModelSpec, writer: Option<ModelSpec>) {
        assert_eq!(plan.tier, tier);
        assert_eq!(plan.command, command);
        assert_eq!(plan.writer, writer);
        let gpu_layers = if tier == Tier::Lite { 0 } else { ALL_GPU_LAYERS };
        assert_eq!(plan.gpu_layers, gpu_layers);
    }

    #[test]
    fn no_gpu_is_lite() {
        assert_plan(plan_for_gpu(None), Tier::Lite, LITE_MODEL, None);
    }

    #[test]
    fn four_gb_is_lite() {
        assert_plan(plan_for_gpu(Some(4 * GB)), Tier::Lite, LITE_MODEL, None);
    }

    #[test]
    fn just_below_the_tolerance_is_lite() {
        // 5900 MiB is under the Qwen3-8B measurement (5939 MiB) and under the threshold.
        assert_plan(plan_for_gpu(Some(5900)), Tier::Lite, LITE_MODEL, None);
    }

    #[test]
    fn six_gb_is_full() {
        assert_plan(plan_for_gpu(Some(6144)), Tier::Full, QWEN3_8B, None);
    }

    #[test]
    fn eight_gb_is_dual_with_small_command_model() {
        assert_plan(plan_for_gpu(Some(8 * GB)), Tier::Dual, QWEN3_4B, Some(JAIS_3_3_7B));
    }

    #[test]
    fn just_below_12_gb_is_dual_with_small_command_model() {
        let mib = (11.9 * GB as f64) as u64;
        assert!(mib < 12 * GB);
        assert_plan(plan_for_gpu(Some(mib)), Tier::Dual, QWEN3_4B, Some(JAIS_3_3_7B));
        assert_plan(plan_for_gpu(Some(12 * GB - 1)), Tier::Dual, QWEN3_4B, Some(JAIS_3_3_7B));
    }

    #[test]
    fn twelve_gb_is_dual_with_large_command_model() {
        assert_plan(plan_for_gpu(Some(12 * GB)), Tier::Dual, QWEN3_8B, Some(JAIS_3_3_7B));
    }

    #[test]
    fn twenty_four_gb_is_dual_with_large_command_model() {
        assert_plan(plan_for_gpu(Some(24 * GB)), Tier::Dual, QWEN3_8B, Some(JAIS_3_3_7B));
    }

    #[test]
    fn card_reporting_slightly_under_8_gb_is_not_dual() {
        // RTX 4060 8 GB cards report 8188 MiB; the Dual threshold is exact.
        assert_eq!(plan_for_gpu(Some(8188)).tier, Tier::Full);
    }

    #[test]
    fn user_choice_is_not_blocked() {
        assert_plan(plan_for(Tier::Full, None), Tier::Full, QWEN3_8B, None);
        assert_plan(plan_for(Tier::Dual, None), Tier::Dual, QWEN3_4B, Some(JAIS_3_3_7B));
        assert_plan(plan_for(Tier::Dual, Some(2 * GB)), Tier::Dual, QWEN3_4B, Some(JAIS_3_3_7B));
        assert_plan(plan_for(Tier::Lite, Some(24 * GB)), Tier::Lite, LITE_MODEL, None);
    }

    #[test]
    fn measured_figures_match_the_pairs() {
        let gib = |m: u64| m as f64 / GB as f64;
        assert!((gib(QWEN3_8B.memory_mib + JAIS_3_3_7B.memory_mib) - 9.8).abs() < 0.05);
        assert!((gib(QWEN3_4B.memory_mib + JAIS_3_3_7B.memory_mib) - 7.7).abs() < 0.05);
        assert!(QWEN3_4B.measured && QWEN3_8B.measured && JAIS_3_3_7B.measured);
        assert!(!LITE_MODEL.measured);
        assert_eq!(LITE_MODEL.file, QWEN3_4B.file);
    }
}
