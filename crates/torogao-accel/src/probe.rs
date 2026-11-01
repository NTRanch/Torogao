//! Capability probing (design section 5.1).
//!
//! Two layers, deliberately separated: **pure parsers** that
//! turn tool output into typed records (fixture-tested against
//! real captures), and a **bounded runner** that executes the
//! tools. A probe that cannot run — tool missing, timeout,
//! unparseable output — becomes a [`ProbeSummary`] with a typed
//! status in the findings. It never becomes an error the app
//! can crash on, and never a fabricated fact.

use std::fmt;
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::bounded_text;
use crate::capability::{
    AdapterBackend, AdapterRecord, CAPABILITY_SCHEMA_ID, CapabilityReport, CudaRecord, DeviceKind,
    GlRecord, IcdRecord, IcdVendor, OpenClPlatform, SessionKind, SessionRecord,
};
use crate::limits;

/// Standard Vulkan ICD directories, in loader search order.
/// The first directory that reads is the one reported.
pub const VULKAN_ICD_DIRS: [&str; 2] = ["/usr/share/vulkan/icd.d", "/etc/vulkan/icd.d"];

/// Standard OpenCL vendor directory.
pub const OPENCL_VENDOR_DIR: &str = "/etc/OpenCL/vendors";

/// How a probe ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProbeStatus {
    /// The probe ran and its output parsed.
    Found,
    /// The tool or directory does not exist on this machine.
    ToolMissing,
    /// The tool ran but failed, exited non-zero, or its output
    /// did not parse.
    ToolFailed,
    /// The probe exceeded its deadline and was killed.
    TimedOut,
    /// The probe's output exceeded the capture bound; the
    /// captured prefix was still usable.
    OutputTruncated,
}

/// Which probe a summary belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProbeKind {
    VulkanInfo,
    NvidiaSmi,
    ClInfo,
    GlxInfo,
    VulkanIcdDir,
    OpenClVendorDir,
    SessionEnv,
}

/// One probe's outcome, carried in the capability report so
/// the report shows its own evidence quality.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeSummary {
    /// The probe.
    pub probe: ProbeKind,
    /// How it ended.
    pub status: ProbeStatus,
    /// Bounded human-readable detail.
    pub detail: String,
}

impl ProbeSummary {
    fn new(probe: ProbeKind, status: ProbeStatus, detail: String) -> Self {
        Self {
            probe,
            status,
            detail: bounded_text(&detail),
        }
    }
}

/// A parse failure. Parsers refuse typed rather than guessing
/// at malformed tool output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeError {
    /// `vulkaninfo` output had no `Devices:` section at all.
    NoDeviceSection,
    /// A required field was absent from otherwise plausible
    /// output.
    MissingField { field: &'static str },
}

impl ProbeError {
    /// The stable refusal code for this error.
    pub fn code(&self) -> &'static str {
        match self {
            Self::NoDeviceSection => "TG-ACCEL-PROBE-NO-DEVICE-SECTION",
            Self::MissingField { .. } => "TG-ACCEL-PROBE-MISSING-FIELD",
        }
    }
}

impl fmt::Display for ProbeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoDeviceSection => write!(f, "vulkaninfo output has no Devices section"),
            Self::MissingField { field } => write!(f, "probe output is missing field {field}"),
        }
    }
}

impl std::error::Error for ProbeError {}

/// One Vulkan device as `vulkaninfo --summary` reports it, in
/// enumeration order (order is a section 7 fact: GPU0 is what
/// an unpinned consumer gets first).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VulkanDevice {
    /// Device display name.
    pub name: String,
    /// Device kind, mapped from `deviceType`.
    pub device_kind: DeviceKind,
    /// `driverName`, e.g. "NVIDIA" or "Intel open-source Mesa
    /// driver".
    pub driver_name: String,
    /// `driverInfo`, e.g. "615.78.08" or "Mesa 26.2.5-arch1.1".
    pub driver_info: String,
    /// Vulkan API version string, e.g. "1.4.351".
    pub api_version: String,
}

fn device_kind_from_type(device_type: &str) -> DeviceKind {
    if device_type.contains("INTEGRATED") {
        DeviceKind::IntegratedGpu
    } else if device_type.contains("DISCRETE") {
        DeviceKind::DiscreteGpu
    } else if device_type.contains("VIRTUAL") {
        DeviceKind::VirtualGpu
    } else if device_type.contains("CPU") {
        DeviceKind::Cpu
    } else {
        DeviceKind::Other
    }
}

/// Parses the `Devices:` section of `vulkaninfo --summary`.
/// Unknown keys and sections are ignored; a block without a
/// `deviceName` is skipped (it is not a device record), and a
/// document with no `Devices:` section refuses.
pub fn parse_vulkaninfo_summary(text: &str) -> Result<Vec<VulkanDevice>, ProbeError> {
    let mut devices = Vec::new();
    let mut in_devices = false;
    let mut current: Vec<(String, String)> = Vec::new();
    let flush = |fields: &[(String, String)], devices: &mut Vec<VulkanDevice>| {
        let get = |key: &str| {
            fields
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.clone())
        };
        if let Some(name) = get("deviceName") {
            devices.push(VulkanDevice {
                name: bounded_text(&name),
                device_kind: get("deviceType")
                    .map(|t| device_kind_from_type(&t))
                    .unwrap_or(DeviceKind::Other),
                driver_name: bounded_text(&get("driverName").unwrap_or_default()),
                driver_info: bounded_text(&get("driverInfo").unwrap_or_default()),
                api_version: bounded_text(&get("apiVersion").unwrap_or_default()),
            });
        }
    };
    for line in text.lines() {
        if line.trim() == "Devices:" {
            in_devices = true;
            continue;
        }
        if !in_devices {
            continue;
        }
        let trimmed = line.trim_start();
        if trimmed.starts_with("GPU") && trimmed.ends_with(':') && !trimmed.contains('=') {
            flush(&current, &mut devices);
            current = Vec::new();
            continue;
        }
        if let Some((key, value)) = trimmed.split_once('=') {
            current.push((key.trim().to_owned(), value.trim().to_owned()));
        }
    }
    if !in_devices {
        return Err(ProbeError::NoDeviceSection);
    }
    flush(&current, &mut devices);
    devices.truncate(limits::ADAPTERS_MAX);
    Ok(devices)
}

/// The facts the `nvidia-smi` banner header carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NvidiaSmiHeader {
    /// Driver version, e.g. "615.78.08".
    pub driver_version: String,
    /// CUDA version the driver supports, e.g. "13.4". The
    /// banner spells this "CUDA Version" on older drivers and
    /// "CUDA UMD Version" on current ones; both parse.
    pub cuda_version: Option<String>,
}

/// Parses the `nvidia-smi` banner header line.
pub fn parse_nvidia_smi_header(text: &str) -> Result<NvidiaSmiHeader, ProbeError> {
    for line in text.lines() {
        if !line.contains("NVIDIA-SMI") {
            continue;
        }
        let tokens: Vec<&str> = line.split_whitespace().collect();
        let Some(pos) = tokens.iter().position(|t| *t == "NVIDIA-SMI") else {
            continue;
        };
        let Some(driver) = tokens.get(pos + 1) else {
            continue;
        };
        // The CUDA version is the token after the first
        // "Version:" that follows the "CUDA" token (the banner
        // also carries an earlier "KMD Version:" for the kernel
        // driver, which must not be picked up).
        let cuda_version = tokens.iter().position(|t| *t == "CUDA").and_then(|start| {
            tokens
                .iter()
                .enumerate()
                .skip(start)
                .find(|(_, t)| **t == "Version:")
                .and_then(|(idx, _)| tokens.get(idx + 1))
                .map(|v| bounded_text(v.trim_matches('|')))
        });
        return Ok(NvidiaSmiHeader {
            driver_version: bounded_text(driver),
            cuda_version,
        });
    }
    Err(ProbeError::MissingField {
        field: "NVIDIA-SMI banner",
    })
}

/// Parses `nvidia-smi --query-gpu=name,driver_version
/// --format=csv,noheader` output into (name, driver version)
/// pairs.
pub fn parse_nvidia_smi_query_csv(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| {
            let (name, driver) = line.rsplit_once(", ")?;
            Some((bounded_text(name.trim()), bounded_text(driver.trim())))
        })
        .collect()
}

/// Parses `clinfo -l` output: `Platform #N: <name>` lines with
/// tree-prefixed `Device #N: <name>` lines beneath each.
pub fn parse_clinfo_list(text: &str) -> Vec<OpenClPlatform> {
    let mut platforms: Vec<OpenClPlatform> = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("Platform #") {
            if let Some((_, name)) = rest.split_once(": ") {
                platforms.push(OpenClPlatform {
                    name: bounded_text(name.trim()),
                    devices: Vec::new(),
                });
            }
        } else if line.contains("Device #") {
            if let (Some((_, name)), Some(platform)) = (line.split_once(": "), platforms.last_mut())
            {
                platform.devices.push(bounded_text(name.trim()));
            }
        }
        if platforms.len() >= limits::OPENCL_PLATFORMS_MAX {
            break;
        }
    }
    platforms
}

/// Parses `glxinfo -B` output into the GL record. The Mesa
/// version is extracted from the version string when it
/// carries one.
pub fn parse_glxinfo(text: &str) -> Result<GlRecord, ProbeError> {
    let mut renderer = None;
    let mut version_string = None;
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("OpenGL renderer string:") {
            renderer = Some(bounded_text(value.trim()));
        } else if let Some(value) = line.strip_prefix("OpenGL version string:") {
            version_string = Some(bounded_text(value.trim()));
        }
    }
    let renderer = renderer.ok_or(ProbeError::MissingField {
        field: "OpenGL renderer string",
    })?;
    let version_string = version_string.ok_or(ProbeError::MissingField {
        field: "OpenGL version string",
    })?;
    let mesa_version = version_string
        .split_once("Mesa ")
        .and_then(|(_, rest)| rest.split_whitespace().next())
        .map(bounded_text);
    Ok(GlRecord {
        renderer,
        version_string,
        mesa_version,
    })
}

/// Classifies a Vulkan ICD file name by vendor.
pub fn classify_vulkan_icd(file_name: &str) -> IcdVendor {
    match file_name {
        "nvidia_icd.json" => IcdVendor::NvidiaProprietary,
        "intel_icd.json" | "intel_hasvk_icd.json" => IcdVendor::IntelMesa,
        "radeon_icd.json" => IcdVendor::AmdMesa,
        "lvp_icd.json" => IcdVendor::LavapipeMesa,
        "nouveau_icd.json" => IcdVendor::NouveauMesa,
        _ => IcdVendor::Other,
    }
}

/// Session facts from an environment reader. Injected so tests
/// drive it without touching the real environment.
pub fn session_from_env(get: impl Fn(&str) -> Option<String>) -> SessionRecord {
    let present = |key: &str| get(key).is_some_and(|v| !v.is_empty());
    let wayland_display = present("WAYLAND_DISPLAY");
    let x11_display = present("DISPLAY");
    let kind = if wayland_display {
        SessionKind::Wayland
    } else if x11_display {
        SessionKind::X11
    } else {
        SessionKind::Unknown
    };
    SessionRecord {
        kind,
        wayland_display,
        x11_display,
    }
}

/// The outcome of one bounded subprocess run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeRun {
    /// How the run ended.
    pub status: ProbeStatus,
    /// Captured stdout (lossy UTF-8), when any was captured.
    pub stdout: Option<String>,
}

/// Runs a probe command with a deadline and an output cap.
///
/// Contract: fixed argv, no shell; stderr discarded (probe
/// stderr is noise, never evidence); the child is killed at
/// the deadline and reaped; captured stdout is bounded by
/// `output_bytes_max` with truncation reported in the status.
/// The reader thread owns the stdout pipe and is joined before
/// return, so no thread or child outlives the call.
pub fn run_bounded(argv: &[&str], deadline: Duration, output_bytes_max: usize) -> ProbeRun {
    assert!(!argv.is_empty(), "probe argv must name a program");
    let spawned = Command::new(argv[0])
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return ProbeRun {
                status: ProbeStatus::ToolMissing,
                stdout: None,
            };
        }
        Err(_) => {
            return ProbeRun {
                status: ProbeStatus::ToolFailed,
                stdout: None,
            };
        }
    };
    let Some(mut pipe) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return ProbeRun {
            status: ProbeStatus::ToolFailed,
            stdout: None,
        };
    };
    let reader = std::thread::spawn(move || {
        let mut buffer: Vec<u8> = Vec::new();
        let mut chunk = [0_u8; 8192];
        let mut truncated = false;
        loop {
            match pipe.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(read_count) => {
                    let room = output_bytes_max.saturating_sub(buffer.len());
                    if read_count > room {
                        buffer.extend_from_slice(&chunk[..room]);
                        truncated = true;
                        break;
                    }
                    buffer.extend_from_slice(&chunk[..read_count]);
                }
            }
        }
        (buffer, truncated)
    });
    let started = Instant::now();
    let mut status = loop {
        match child.try_wait() {
            Ok(Some(exit)) => {
                break if exit.success() {
                    ProbeStatus::Found
                } else {
                    ProbeStatus::ToolFailed
                };
            }
            Ok(None) => {
                if started.elapsed() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    break ProbeStatus::TimedOut;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(_) => break ProbeStatus::ToolFailed,
        }
    };
    let (buffer, truncated) = reader.join().unwrap_or_default();
    if truncated && status != ProbeStatus::TimedOut {
        // Truncation dominates the exit status: a producer
        // whose reader went away usually dies of SIGPIPE, and
        // that exit code says nothing about the probe — the
        // capped prefix is the fact that matters.
        status = ProbeStatus::OutputTruncated;
    }
    let stdout = if buffer.is_empty() {
        None
    } else {
        Some(String::from_utf8_lossy(&buffer).into_owned())
    };
    ProbeRun { status, stdout }
}

/// Everything the probes found on this machine, before it
/// becomes a [`CapabilityReport`].
#[derive(Debug, Default)]
pub struct ProbeFindings {
    /// Vulkan devices in enumeration order.
    pub vulkan_devices: Vec<VulkanDevice>,
    /// NVIDIA driver version, when found.
    pub nvidia_driver_version: Option<String>,
    /// CUDA version from the `nvidia-smi` banner, when found.
    pub cuda_version: Option<String>,
    /// NVIDIA device names from the query form.
    pub cuda_devices: Vec<String>,
    /// OpenCL platforms with devices.
    pub opencl_platforms: Vec<OpenClPlatform>,
    /// GL facts, when probed.
    pub gl: Option<GlRecord>,
    /// Installed Vulkan ICDs, classified.
    pub vulkan_icds: Vec<IcdRecord>,
    /// Directory the Vulkan ICD listing came from.
    pub vulkan_icd_dir: Option<String>,
    /// Installed OpenCL ICD file names.
    pub opencl_icd_files: Vec<String>,
    /// Session facts.
    pub session: SessionRecord,
    /// Per-probe status trail.
    pub summaries: Vec<ProbeSummary>,
}

impl ProbeFindings {
    /// Converts the findings into the capability report
    /// record. Vulkan devices become the report's Vulkan
    /// adapters; the CUDA record exists exactly when an NVIDIA
    /// stack was found.
    pub fn into_report(self) -> CapabilityReport {
        let adapters = self
            .vulkan_devices
            .iter()
            .map(|device| AdapterRecord {
                name: device.name.clone(),
                backend: AdapterBackend::Vulkan,
                device_kind: device.device_kind,
                driver_name: device.driver_name.clone(),
                driver_info: device.driver_info.clone(),
                software: device.device_kind == DeviceKind::Cpu,
            })
            .collect();
        let cuda = if self.nvidia_driver_version.is_some() || !self.cuda_devices.is_empty() {
            Some(CudaRecord {
                driver_version: self.nvidia_driver_version.unwrap_or_default(),
                cuda_umd_version: self.cuda_version,
                devices: self.cuda_devices,
            })
        } else {
            None
        };
        CapabilityReport {
            schema: CAPABILITY_SCHEMA_ID.to_owned(),
            session: self.session,
            adapters,
            vulkan_icds: self.vulkan_icds,
            vulkan_icd_dir: self.vulkan_icd_dir,
            opencl_icd_files: self.opencl_icd_files,
            opencl_platforms: self.opencl_platforms,
            cuda,
            gl: self.gl,
            probes: self.summaries,
        }
    }
}

/// Lists one directory's file names, sorted. `None` when the
/// directory cannot be read.
fn list_file_names(dir: &str) -> Option<Vec<String>> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut names: Vec<String> = entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    names.sort();
    Some(names)
}

/// Collects capability findings from this machine. Every step
/// is bounded (see [`run_bounded`]) and appends its
/// [`ProbeSummary`]; the function is total — a machine with
/// none of the tools yields findings full of `ToolMissing`
/// summaries and empty facts, which evaluate to the software
/// plan.
pub fn collect_findings() -> ProbeFindings {
    let deadline = Duration::from_millis(limits::PROBE_DEADLINE_MS);
    let cap = limits::PROBE_OUTPUT_BYTES_MAX;
    let session = session_from_env(|key| std::env::var(key).ok());
    let mut findings = ProbeFindings {
        session,
        ..ProbeFindings::default()
    };

    findings.summaries.push(ProbeSummary::new(
        ProbeKind::SessionEnv,
        ProbeStatus::Found,
        format!("session kind {:?}", findings.session.kind),
    ));

    let run = run_bounded(&["vulkaninfo", "--summary"], deadline, cap);
    match (run.status, run.stdout) {
        (ProbeStatus::Found | ProbeStatus::OutputTruncated, Some(text)) => {
            match parse_vulkaninfo_summary(&text) {
                Ok(devices) => {
                    findings.summaries.push(ProbeSummary::new(
                        ProbeKind::VulkanInfo,
                        ProbeStatus::Found,
                        format!("{} Vulkan device(s)", devices.len()),
                    ));
                    findings.vulkan_devices = devices;
                }
                Err(err) => findings.summaries.push(ProbeSummary::new(
                    ProbeKind::VulkanInfo,
                    ProbeStatus::ToolFailed,
                    format!("{}: {err}", err.code()),
                )),
            }
        }
        (status, _) => findings.summaries.push(ProbeSummary::new(
            ProbeKind::VulkanInfo,
            status,
            "vulkaninfo --summary did not produce output".to_owned(),
        )),
    }

    for dir in VULKAN_ICD_DIRS {
        if let Some(names) = list_file_names(dir) {
            findings.vulkan_icds = names
                .iter()
                .filter(|name| name.ends_with(".json"))
                .map(|name| IcdRecord {
                    file_name: bounded_text(name),
                    vendor: classify_vulkan_icd(name),
                })
                .collect();
            findings.vulkan_icd_dir = Some(dir.to_owned());
            findings.summaries.push(ProbeSummary::new(
                ProbeKind::VulkanIcdDir,
                ProbeStatus::Found,
                format!("{} ICD file(s) in {dir}", findings.vulkan_icds.len()),
            ));
            break;
        }
    }
    if findings.vulkan_icd_dir.is_none() {
        findings.summaries.push(ProbeSummary::new(
            ProbeKind::VulkanIcdDir,
            ProbeStatus::ToolMissing,
            "no Vulkan ICD directory found".to_owned(),
        ));
    }

    collect_nvidia(&mut findings, deadline, cap);
    collect_opencl(&mut findings, deadline, cap);

    let run = run_bounded(&["glxinfo", "-B"], deadline, cap);
    match (run.status, run.stdout) {
        (ProbeStatus::Found | ProbeStatus::OutputTruncated, Some(text)) => {
            match parse_glxinfo(&text) {
                Ok(gl) => {
                    findings.summaries.push(ProbeSummary::new(
                        ProbeKind::GlxInfo,
                        ProbeStatus::Found,
                        format!("renderer {}", gl.renderer),
                    ));
                    findings.gl = Some(gl);
                }
                Err(err) => findings.summaries.push(ProbeSummary::new(
                    ProbeKind::GlxInfo,
                    ProbeStatus::ToolFailed,
                    format!("{}: {err}", err.code()),
                )),
            }
        }
        (status, _) => findings.summaries.push(ProbeSummary::new(
            ProbeKind::GlxInfo,
            status,
            "glxinfo -B did not produce output (it needs a display)".to_owned(),
        )),
    }

    findings
}

fn collect_nvidia(findings: &mut ProbeFindings, deadline: Duration, cap: usize) {
    let banner = run_bounded(&["nvidia-smi"], deadline, cap);
    let banner_status = banner.status;
    if let Some(text) = &banner.stdout {
        if let Ok(header) = parse_nvidia_smi_header(text) {
            findings.nvidia_driver_version = Some(header.driver_version);
            findings.cuda_version = header.cuda_version;
        }
    }
    let query = run_bounded(
        &[
            "nvidia-smi",
            "--query-gpu=name,driver_version",
            "--format=csv,noheader",
        ],
        deadline,
        cap,
    );
    if let Some(text) = &query.stdout {
        findings.cuda_devices = parse_nvidia_smi_query_csv(text)
            .into_iter()
            .map(|(name, _)| name)
            .collect();
    }
    let status = if findings.nvidia_driver_version.is_some() {
        ProbeStatus::Found
    } else {
        banner_status
    };
    findings.summaries.push(ProbeSummary::new(
        ProbeKind::NvidiaSmi,
        status,
        format!("{} CUDA device(s)", findings.cuda_devices.len()),
    ));
}

fn collect_opencl(findings: &mut ProbeFindings, deadline: Duration, cap: usize) {
    match list_file_names(OPENCL_VENDOR_DIR) {
        Some(names) => {
            findings.opencl_icd_files = names
                .iter()
                .filter(|name| name.ends_with(".icd"))
                .map(|name| bounded_text(name))
                .collect();
            findings.summaries.push(ProbeSummary::new(
                ProbeKind::OpenClVendorDir,
                ProbeStatus::Found,
                format!(
                    "{} OpenCL ICD file(s) in {OPENCL_VENDOR_DIR}",
                    findings.opencl_icd_files.len()
                ),
            ));
        }
        None => findings.summaries.push(ProbeSummary::new(
            ProbeKind::OpenClVendorDir,
            ProbeStatus::ToolMissing,
            format!("{OPENCL_VENDOR_DIR} not found"),
        )),
    }
    let run = run_bounded(&["clinfo", "-l"], deadline, cap);
    match (run.status, run.stdout) {
        (ProbeStatus::Found | ProbeStatus::OutputTruncated, Some(text)) => {
            findings.opencl_platforms = parse_clinfo_list(&text);
            findings.summaries.push(ProbeSummary::new(
                ProbeKind::ClInfo,
                ProbeStatus::Found,
                format!("{} OpenCL platform(s)", findings.opencl_platforms.len()),
            ));
        }
        (status, _) => findings.summaries.push(ProbeSummary::new(
            ProbeKind::ClInfo,
            status,
            "clinfo -l did not produce output".to_owned(),
        )),
    }
}
