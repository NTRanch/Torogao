//! The capability report and the effective-plan evaluator
//! (design sections 4 and 5).
//!
//! The report is a record: what the machine offers, gathered by
//! [`crate::probe`] (and, in the app, by wgpu adapter
//! enumeration feeding the same shape). The evaluator is a pure
//! function from (settings, report) to the plan the app
//! executes. Every preference the machine cannot honor becomes
//! a [`Degradation`] in the plan — visible in Settings, with a
//! stable code — never a silent fallback and never a crash.

use serde::{Deserialize, Serialize};

use crate::bounded_text;
use crate::limits;
use crate::probe::ProbeSummary;
use crate::settings::{AccelSettings, ComputeIcdPin, PowerPref, RenderBackendPref};

/// Schema id of the capability report record.
pub const CAPABILITY_SCHEMA_ID: &str = "torogao-accel-capability/1";

/// File name of the NVIDIA proprietary Vulkan ICD, as installed
/// under the loader's ICD directory.
pub const NVIDIA_ICD_FILE_NAME: &str = "nvidia_icd.json";

/// Environment variable a Vulkan compute launch uses to pin
/// its ICD (design section 7).
pub const VK_ICD_ENV_VAR: &str = "VK_ICD_FILENAMES";

/// Which graphics API an adapter was enumerated through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AdapterBackend {
    Vulkan,
    OpenGl,
    Metal,
    DirectX12,
    WebGpu,
    Unknown,
}

/// What kind of device an adapter is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeviceKind {
    DiscreteGpu,
    IntegratedGpu,
    VirtualGpu,
    Cpu,
    Other,
}

/// One enumerated graphics adapter. Driver strings are
/// untrusted display data, bounded at record construction by
/// the producers (probe / app enumeration).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterRecord {
    /// Adapter display name, e.g. "NVIDIA GeForce RTX 4070
    /// Laptop GPU".
    pub name: String,
    /// Backend the adapter was enumerated through.
    pub backend: AdapterBackend,
    /// Device kind.
    pub device_kind: DeviceKind,
    /// Driver display name, e.g. "Intel open-source Mesa
    /// driver" or "NVIDIA".
    pub driver_name: String,
    /// Driver version string, e.g. "Mesa 26.2.5-arch1.1" or
    /// "615.78.08".
    pub driver_info: String,
    /// True when the adapter is a software rasterizer
    /// (lavapipe/llvmpipe class).
    pub software: bool,
}

/// The windowing session the app is running in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SessionKind {
    Wayland,
    X11,
    Windows,
    MacOs,
    Browser,
    Headless,
    #[default]
    Unknown,
}

/// Session facts from the process environment. Both display
/// flags are recorded separately from the chosen kind because
/// "Wayland desktop, X11 surface" is exactly the XWayland case
/// and support needs to see it (design section 2.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SessionRecord {
    /// The session kind in use.
    pub kind: SessionKind,
    /// `WAYLAND_DISPLAY` was present and non-empty.
    pub wayland_display: bool,
    /// `DISPLAY` was present and non-empty.
    pub x11_display: bool,
}

/// CUDA facts, present when an NVIDIA driver stack was found.
/// Detection only — the presence of this record is not an
/// adoption of CUDA (design section 2.6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CudaRecord {
    /// NVIDIA driver version, e.g. "615.78.08".
    pub driver_version: String,
    /// CUDA UMD version from the `nvidia-smi` header, when
    /// reported.
    pub cuda_umd_version: Option<String>,
    /// Device display names.
    pub devices: Vec<String>,
}

/// OpenGL facts from `glxinfo`, when the probe ran. The Mesa
/// version is extracted from the version string when present.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlRecord {
    /// `OpenGL renderer string` value.
    pub renderer: String,
    /// `OpenGL version string` value.
    pub version_string: String,
    /// Mesa version parsed from the version string, e.g.
    /// "26.2.5-arch1.1".
    pub mesa_version: Option<String>,
}

/// One OpenCL platform with its devices, as `clinfo -l`
/// reports them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenClPlatform {
    /// Platform display name, e.g. "NVIDIA CUDA".
    pub name: String,
    /// Device display names under the platform.
    pub devices: Vec<String>,
}

/// Which vendor an installed Vulkan ICD belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IcdVendor {
    NvidiaProprietary,
    IntelMesa,
    AmdMesa,
    LavapipeMesa,
    NouveauMesa,
    Other,
}

/// One installed Vulkan ICD file, classified.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IcdRecord {
    /// Bare file name, e.g. "nvidia_icd.json".
    pub file_name: String,
    /// Classified vendor.
    pub vendor: IcdVendor,
}

/// The capability report record (design section 5.2). Produced
/// at startup; its probe summary trail shows its own evidence
/// quality.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityReport {
    /// Schema id; equals [`CAPABILITY_SCHEMA_ID`].
    pub schema: String,
    /// Session facts.
    pub session: SessionRecord,
    /// Enumerated adapters (Vulkan probe order preserved; the
    /// app appends its wgpu enumeration in its own order).
    pub adapters: Vec<AdapterRecord>,
    /// Installed Vulkan ICDs.
    pub vulkan_icds: Vec<IcdRecord>,
    /// Directory the ICD listing came from, when listed.
    pub vulkan_icd_dir: Option<String>,
    /// OpenCL ICD file names installed, bare names as listed.
    pub opencl_icd_files: Vec<String>,
    /// OpenCL platforms with devices, when `clinfo` ran.
    pub opencl_platforms: Vec<OpenClPlatform>,
    /// CUDA facts, when found.
    pub cuda: Option<CudaRecord>,
    /// GL facts, when probed.
    pub gl: Option<GlRecord>,
    /// Per-probe status trail.
    pub probes: Vec<ProbeSummary>,
}

impl CapabilityReport {
    /// An empty report: the honest value when every probe
    /// failed or none has run yet. Evaluation against it yields
    /// the software plan with the reasons attached.
    pub fn empty() -> Self {
        Self {
            schema: CAPABILITY_SCHEMA_ID.to_owned(),
            session: SessionRecord::default(),
            adapters: Vec::new(),
            vulkan_icds: Vec::new(),
            vulkan_icd_dir: None,
            opencl_icd_files: Vec::new(),
            opencl_platforms: Vec::new(),
            cuda: None,
            gl: None,
            probes: Vec::new(),
        }
    }

    /// True when the machine is hybrid: both an integrated and
    /// a discrete GPU adapter are present. This is the shape
    /// the section 7 safety rule exists for.
    pub fn is_hybrid(&self) -> bool {
        let has = |kind: DeviceKind| {
            self.adapters
                .iter()
                .any(|a| !a.software && a.device_kind == kind)
        };
        has(DeviceKind::IntegratedGpu) && has(DeviceKind::DiscreteGpu)
    }

    /// The pinned NVIDIA ICD path, built from the directory and
    /// file name the probe actually found — data, not a
    /// hardcoded string (design section 7).
    pub fn nvidia_icd_path(&self) -> Option<String> {
        let dir = self.vulkan_icd_dir.as_ref()?;
        let found = self
            .vulkan_icds
            .iter()
            .any(|icd| icd.file_name == NVIDIA_ICD_FILE_NAME);
        found.then(|| format!("{dir}/{NVIDIA_ICD_FILE_NAME}"))
    }
}

/// How badly a preference went unmet (design section 5.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DegradationSeverity {
    /// A different backend/adapter than asked is in use.
    FallbackApplied,
    /// A requested feature is off.
    FeatureOff,
    /// Nothing is off, but the user should know.
    Note,
}

/// One unmet or adjusted preference, rendered verbatim in
/// Settings. `code` is a stable `TG-ACCEL-DEG-*` /
/// `TG-ACCEL-NOTE-*` string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Degradation {
    /// The feature or control the entry is about.
    pub feature: String,
    /// Severity.
    pub severity: DegradationSeverity,
    /// Stable machine-readable code.
    pub code: String,
    /// User-readable reason, bounded.
    pub reason: String,
}

impl Degradation {
    fn new(feature: &str, severity: DegradationSeverity, code: &str, reason: String) -> Self {
        Self {
            feature: feature.to_owned(),
            severity,
            code: code.to_owned(),
            reason: bounded_text(&reason),
        }
    }
}

/// The rendering backend a plan actually uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EffectiveRenderBackend {
    Vulkan,
    OpenGl,
    Software,
}

/// The rendering half of the effective plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderPlan {
    /// Backend in use.
    pub backend: EffectiveRenderBackend,
    /// Chosen adapter display name, when one was chosen.
    pub adapter_name: Option<String>,
    /// True when rendering is GPU-accelerated.
    pub accelerated: bool,
}

/// A compute backend a plan may select.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ComputeBackend {
    Vulkan,
    Cuda,
    OpenCl,
}

/// The compute half of the effective plan. `env_pairs` are set
/// only around compute launches, never for the whole process
/// (design section 8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComputePlan {
    /// Whether gated compute workloads may run.
    pub enabled: bool,
    /// Backend in use, when enabled.
    pub backend: Option<ComputeBackend>,
    /// Device display name, when one was chosen.
    pub device_name: Option<String>,
    /// Environment pairs for compute launches (the section 7
    /// ICD pin lives here).
    pub env_pairs: Vec<(String, String)>,
}

impl ComputePlan {
    fn disabled() -> Self {
        Self {
            enabled: false,
            backend: None,
            device_name: None,
            env_pairs: Vec::new(),
        }
    }
}

/// What the app executes: render plan + compute plan + every
/// degradation, in one value (design section 4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectivePlan {
    /// Rendering decision.
    pub render: RenderPlan,
    /// Compute decision.
    pub compute: ComputePlan,
    /// All unmet/adjusted preferences, user-visible.
    pub degradations: Vec<Degradation>,
}

/// Evaluates settings against the capability report. Pure and
/// total: any (settings, report) pair yields a plan, with the
/// software configuration as the floor. Assumes `settings` has
/// passed validation; contradictory leftovers degrade to the
/// safe reading rather than panicking.
pub fn evaluate(settings: &AccelSettings, report: &CapabilityReport) -> EffectivePlan {
    let mut degradations = Vec::new();
    let render = plan_render(settings, report, &mut degradations);
    let compute = plan_compute(settings, report, &mut degradations);
    degradations.truncate(limits::DEGRADATIONS_MAX);
    EffectivePlan {
        render,
        compute,
        degradations,
    }
}

/// Adapter preference rank within one backend: lower wins.
/// Power preference orders device kinds; name breaks remaining
/// ties so selection is deterministic across runs and reports.
fn adapter_rank(kind: DeviceKind, power: PowerPref) -> u8 {
    match power {
        PowerPref::LowPower => match kind {
            DeviceKind::IntegratedGpu => 0,
            DeviceKind::VirtualGpu => 1,
            DeviceKind::DiscreteGpu => 2,
            DeviceKind::Other => 3,
            DeviceKind::Cpu => 4,
        },
        PowerPref::Auto | PowerPref::HighPerformance => match kind {
            DeviceKind::DiscreteGpu => 0,
            DeviceKind::IntegratedGpu => 1,
            DeviceKind::VirtualGpu => 2,
            DeviceKind::Other => 3,
            DeviceKind::Cpu => 4,
        },
    }
}

/// Picks the best non-software adapter of one backend, if any.
fn pick_adapter(
    report: &CapabilityReport,
    backend: AdapterBackend,
    power: PowerPref,
) -> Option<&AdapterRecord> {
    let mut candidates: Vec<&AdapterRecord> = report
        .adapters
        .iter()
        .filter(|a| a.backend == backend && !a.software)
        .collect();
    candidates.sort_by(|a, b| {
        adapter_rank(a.device_kind, power)
            .cmp(&adapter_rank(b.device_kind, power))
            .then_with(|| a.name.cmp(&b.name))
    });
    candidates.into_iter().next()
}

/// The software render plan: first software adapter as the
/// display name when one was enumerated.
fn software_plan(report: &CapabilityReport) -> RenderPlan {
    let adapter_name = report
        .adapters
        .iter()
        .filter(|a| a.software)
        .map(|a| a.name.clone())
        .min();
    RenderPlan {
        backend: EffectiveRenderBackend::Software,
        adapter_name,
        accelerated: false,
    }
}

fn gpu_plan(backend: EffectiveRenderBackend, adapter: &AdapterRecord) -> RenderPlan {
    RenderPlan {
        backend,
        adapter_name: Some(adapter.name.clone()),
        accelerated: true,
    }
}

fn plan_render(
    settings: &AccelSettings,
    report: &CapabilityReport,
    degradations: &mut Vec<Degradation>,
) -> RenderPlan {
    if !settings.master_enabled || settings.render_backend == RenderBackendPref::Software {
        return software_plan(report);
    }
    let power = settings.power_preference;
    match settings.render_backend {
        RenderBackendPref::Auto => {
            if let Some(a) = pick_adapter(report, AdapterBackend::Vulkan, power) {
                return gpu_plan(EffectiveRenderBackend::Vulkan, a);
            }
            if let Some(a) = pick_adapter(report, AdapterBackend::OpenGl, power) {
                return gpu_plan(EffectiveRenderBackend::OpenGl, a);
            }
            degradations.push(Degradation::new(
                "render_backend",
                DegradationSeverity::FallbackApplied,
                "TG-ACCEL-DEG-NO-GPU-ADAPTER",
                "no GPU adapter was found; using software rendering".to_owned(),
            ));
            software_plan(report)
        }
        RenderBackendPref::Vulkan => plan_explicit(
            report,
            power,
            AdapterBackend::Vulkan,
            EffectiveRenderBackend::Vulkan,
            "Vulkan",
            degradations,
        ),
        RenderBackendPref::OpenGl => plan_explicit(
            report,
            power,
            AdapterBackend::OpenGl,
            EffectiveRenderBackend::OpenGl,
            "OpenGL",
            degradations,
        ),
        RenderBackendPref::Software => software_plan(report),
    }
}

/// An explicit backend preference: honor it, else fall back to
/// the other GPU backend, else software — each step recorded.
fn plan_explicit(
    report: &CapabilityReport,
    power: PowerPref,
    backend: AdapterBackend,
    effective: EffectiveRenderBackend,
    label: &str,
    degradations: &mut Vec<Degradation>,
) -> RenderPlan {
    if let Some(a) = pick_adapter(report, backend, power) {
        return gpu_plan(effective, a);
    }
    let (other_backend, other_effective, other_label) = match backend {
        AdapterBackend::Vulkan => (
            AdapterBackend::OpenGl,
            EffectiveRenderBackend::OpenGl,
            "OpenGL",
        ),
        _ => (
            AdapterBackend::Vulkan,
            EffectiveRenderBackend::Vulkan,
            "Vulkan",
        ),
    };
    if let Some(a) = pick_adapter(report, other_backend, power) {
        degradations.push(Degradation::new(
            "render_backend",
            DegradationSeverity::FallbackApplied,
            "TG-ACCEL-DEG-BACKEND-FALLBACK",
            format!(
                "{label} was requested but no {label} GPU adapter was found; using {other_label}"
            ),
        ));
        return gpu_plan(other_effective, a);
    }
    degradations.push(Degradation::new(
        "render_backend",
        DegradationSeverity::FallbackApplied,
        "TG-ACCEL-DEG-BACKEND-FALLBACK",
        format!("{label} was requested but no GPU adapter was found; using software rendering"),
    ));
    software_plan(report)
}

/// Resolves the Vulkan compute ICD pin to the environment
/// pairs for a compute launch. Err carries the reason the
/// Vulkan path cannot serve; the caller falls back or reports.
/// The hybrid-safety override of `SystemDefault` is recorded
/// here, where it happens (design section 7).
fn resolve_vulkan_pin(
    settings: &AccelSettings,
    report: &CapabilityReport,
    degradations: &mut Vec<Degradation>,
) -> Result<Vec<(String, String)>, String> {
    let hybrid = report.is_hybrid();
    let pin_pair = |path: String| vec![(VK_ICD_ENV_VAR.to_owned(), path)];
    match settings.compute_icd_pin {
        ComputeIcdPin::NvidiaOnly => report.nvidia_icd_path().map(pin_pair).ok_or_else(|| {
            "NVIDIA-only pin was requested but nvidia_icd.json is not installed".to_owned()
        }),
        ComputeIcdPin::Auto => {
            if !hybrid {
                return Ok(Vec::new());
            }
            report.nvidia_icd_path().map(pin_pair).ok_or_else(|| {
                "hybrid machine: nvidia_icd.json not found, refusing unpinned Vulkan compute"
                    .to_owned()
            })
        }
        ComputeIcdPin::SystemDefault => {
            if !hybrid {
                return Ok(Vec::new());
            }
            let path = report.nvidia_icd_path().ok_or_else(|| {
                "hybrid machine: nvidia_icd.json not found, refusing unpinned Vulkan compute"
                    .to_owned()
            })?;
            degradations.push(Degradation::new(
                "compute_icd_pin",
                DegradationSeverity::Note,
                "TG-ACCEL-NOTE-PIN-OVERRIDE",
                "system-default ICD selection is overridden by the hybrid-safety pin \
                 for compute launches (design section 7)"
                    .to_owned(),
            ));
            Ok(pin_pair(path))
        }
    }
}

fn plan_compute(
    settings: &AccelSettings,
    report: &CapabilityReport,
    degradations: &mut Vec<Degradation>,
) -> ComputePlan {
    if !settings.master_enabled || !settings.gpu_compute_enabled {
        return ComputePlan::disabled();
    }
    let mut failures: Vec<String> = Vec::new();

    // First candidate: Vulkan compute, subject to the pin law.
    match pick_adapter(report, AdapterBackend::Vulkan, PowerPref::HighPerformance) {
        Some(adapter) => match resolve_vulkan_pin(settings, report, degradations) {
            Ok(env_pairs) => {
                return ComputePlan {
                    enabled: true,
                    backend: Some(ComputeBackend::Vulkan),
                    device_name: Some(adapter.name.clone()),
                    env_pairs,
                };
            }
            Err(reason) => failures.push(format!("Vulkan compute unavailable: {reason}")),
        },
        None => failures.push("Vulkan compute unavailable: no Vulkan GPU adapter".to_owned()),
    }

    // Second candidate: CUDA. Device selection is explicit in
    // the CUDA model, so no ICD ambiguity exists to fence.
    if let Some(cuda) = &report.cuda {
        if let Some(device) = cuda.devices.first() {
            degradations.push(Degradation::new(
                "gpu_compute",
                DegradationSeverity::Note,
                "TG-ACCEL-NOTE-COMPUTE-FALLBACK",
                format!("using CUDA for compute; {}", failures.join("; ")),
            ));
            return ComputePlan {
                enabled: true,
                backend: Some(ComputeBackend::Cuda),
                device_name: Some(device.clone()),
                env_pairs: Vec::new(),
            };
        }
    }
    failures.push("CUDA unavailable: no CUDA device was found".to_owned());

    // Third candidate: OpenCL, first platform with a device.
    for platform in &report.opencl_platforms {
        if let Some(device) = platform.devices.first() {
            degradations.push(Degradation::new(
                "gpu_compute",
                DegradationSeverity::Note,
                "TG-ACCEL-NOTE-COMPUTE-FALLBACK",
                format!("using OpenCL for compute; {}", failures.join("; ")),
            ));
            return ComputePlan {
                enabled: true,
                backend: Some(ComputeBackend::OpenCl),
                device_name: Some(device.clone()),
                env_pairs: Vec::new(),
            };
        }
    }
    failures.push("OpenCL unavailable: no OpenCL device was found".to_owned());

    degradations.push(Degradation::new(
        "gpu_compute",
        DegradationSeverity::FeatureOff,
        "TG-ACCEL-DEG-COMPUTE-UNAVAILABLE",
        format!(
            "GPU compute was requested but cannot serve: {}",
            failures.join("; ")
        ),
    ));
    ComputePlan::disabled()
}
