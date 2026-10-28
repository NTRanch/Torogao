//! Probe parser tests against recorded fixtures (authentic
//! primo captures; see the program log for provenance) plus
//! the bounded runner's behavior against real system tools.

use std::time::Duration;

use torogao_accel::probe::{
    classify_vulkan_icd, parse_clinfo_list, parse_glxinfo, parse_nvidia_smi_header,
    parse_nvidia_smi_query_csv, parse_vulkaninfo_summary, run_bounded, session_from_env,
};
use torogao_accel::{DeviceKind, IcdVendor, ProbeStatus, SessionKind};

const VULKANINFO: &str = include_str!("fixtures/vulkaninfo-summary-primo.txt");
const NVIDIA_SMI: &str = include_str!("fixtures/nvidia-smi-primo.txt");
const NVIDIA_SMI_QUERY: &str = include_str!("fixtures/nvidia-smi-query-primo.txt");
const CLINFO: &str = include_str!("fixtures/clinfo-list-primo.txt");
const GLXINFO_MODELED: &str = include_str!("fixtures/glxinfo-b-modeled.txt");

#[test]
fn vulkaninfo_fixture_parses_primo_devices_in_order() {
    let devices = parse_vulkaninfo_summary(VULKANINFO).expect("fixture parses");
    assert_eq!(devices.len(), 4);

    // GPU0 is the integrated Intel device: the enumeration
    // order is the section 7 hazard fact.
    assert_eq!(devices[0].name, "Intel(R) Graphics (RPL-P)");
    assert_eq!(devices[0].device_kind, DeviceKind::IntegratedGpu);
    assert_eq!(devices[0].driver_name, "Intel open-source Mesa driver");
    assert_eq!(devices[0].driver_info, "Mesa 26.2.5-arch1.1");

    assert_eq!(devices[1].name, "NVIDIA GeForce RTX 4070 Laptop GPU");
    assert_eq!(devices[1].device_kind, DeviceKind::DiscreteGpu);
    assert_eq!(devices[1].driver_name, "NVIDIA");
    assert_eq!(devices[1].driver_info, "615.78.08");

    assert_eq!(devices[2].name, "NVIDIA RTX A6000");
    assert_eq!(devices[2].device_kind, DeviceKind::DiscreteGpu);

    assert_eq!(devices[3].name, "llvmpipe (LLVM 23.1.3, 256 bits)");
    assert_eq!(devices[3].device_kind, DeviceKind::Cpu);
}

#[test]
fn vulkaninfo_garbage_refuses_typed() {
    let err = parse_vulkaninfo_summary("not a vulkaninfo output\n").expect_err("must refuse");
    assert_eq!(err.code(), "TG-ACCEL-PROBE-NO-DEVICE-SECTION");
}

#[test]
fn nvidia_smi_header_parses_driver_and_cuda_umd() {
    let header = parse_nvidia_smi_header(NVIDIA_SMI).expect("header parses");
    assert_eq!(header.driver_version, "615.78.08");
    assert_eq!(header.cuda_version.as_deref(), Some("13.4"));
}

#[test]
fn nvidia_smi_header_parses_the_older_banner_spelling() {
    let banner = "| NVIDIA-SMI 550.54.14   Driver Version: 550.54.14   CUDA Version: 12.4   |";
    let header = parse_nvidia_smi_header(banner).expect("header parses");
    assert_eq!(header.driver_version, "550.54.14");
    assert_eq!(header.cuda_version.as_deref(), Some("12.4"));
}

#[test]
fn nvidia_smi_query_csv_parses_names_and_drivers() {
    let pairs = parse_nvidia_smi_query_csv(NVIDIA_SMI_QUERY);
    assert_eq!(
        pairs,
        vec![
            (
                "NVIDIA GeForce RTX 4070 Laptop GPU".to_owned(),
                "615.78.08".to_owned()
            ),
            ("NVIDIA RTX A6000".to_owned(), "615.78.08".to_owned()),
        ]
    );
}

#[test]
fn clinfo_fixture_parses_platforms_and_devices() {
    let platforms = parse_clinfo_list(CLINFO);
    assert_eq!(platforms.len(), 5);
    assert_eq!(platforms[0].name, "Portable Computing Language");
    assert_eq!(platforms[0].devices.len(), 3);
    assert_eq!(platforms[1].name, "Intel(R) OpenCL Graphics (integrated)");
    assert_eq!(platforms[2].name, "NVIDIA CUDA");
    assert_eq!(
        platforms[2].devices,
        vec![
            "NVIDIA RTX A6000".to_owned(),
            "NVIDIA GeForce RTX 4070 Laptop GPU".to_owned()
        ]
    );
    assert_eq!(platforms[3].name, "rusticl");
    assert!(platforms[3].devices.is_empty());
    assert_eq!(platforms[4].name, "clvk");
    assert_eq!(platforms[4].devices.len(), 4);
}

#[test]
fn glxinfo_modeled_fixture_parses_renderer_and_mesa() {
    let gl = parse_glxinfo(GLXINFO_MODELED).expect("fixture parses");
    assert_eq!(gl.renderer, "Mesa Intel(R) Graphics (RPL-P)");
    assert_eq!(gl.mesa_version.as_deref(), Some("26.2.5-arch1.1"));
    assert!(gl.version_string.contains("Mesa 26.2.5-arch1.1"));
}

#[test]
fn glxinfo_missing_renderer_refuses_typed() {
    let err = parse_glxinfo("OpenGL version string: 4.6\n").expect_err("must refuse");
    assert_eq!(err.code(), "TG-ACCEL-PROBE-MISSING-FIELD");
}

#[test]
fn vulkan_icd_classification_matches_primo_listing() {
    assert_eq!(
        classify_vulkan_icd("nvidia_icd.json"),
        IcdVendor::NvidiaProprietary
    );
    assert_eq!(classify_vulkan_icd("intel_icd.json"), IcdVendor::IntelMesa);
    assert_eq!(
        classify_vulkan_icd("intel_hasvk_icd.json"),
        IcdVendor::IntelMesa
    );
    assert_eq!(classify_vulkan_icd("radeon_icd.json"), IcdVendor::AmdMesa);
    assert_eq!(classify_vulkan_icd("lvp_icd.json"), IcdVendor::LavapipeMesa);
    assert_eq!(
        classify_vulkan_icd("nouveau_icd.json"),
        IcdVendor::NouveauMesa
    );
    assert_eq!(classify_vulkan_icd("asahi_icd.json"), IcdVendor::Other);
    assert_eq!(classify_vulkan_icd("virtio_icd.json"), IcdVendor::Other);
}

#[test]
fn session_from_env_reads_the_xwayland_case() {
    let both = session_from_env(|key| match key {
        "WAYLAND_DISPLAY" => Some("wayland-1".to_owned()),
        "DISPLAY" => Some(":0".to_owned()),
        _ => None,
    });
    assert_eq!(both.kind, SessionKind::Wayland);
    assert!(both.wayland_display && both.x11_display);

    let x11_only = session_from_env(|key| match key {
        "DISPLAY" => Some(":0".to_owned()),
        _ => None,
    });
    assert_eq!(x11_only.kind, SessionKind::X11);
    assert!(!x11_only.wayland_display && x11_only.x11_display);

    let neither = session_from_env(|_| None);
    assert_eq!(neither.kind, SessionKind::Unknown);

    let empty_wayland = session_from_env(|key| match key {
        "WAYLAND_DISPLAY" => Some(String::new()),
        "DISPLAY" => Some(":0".to_owned()),
        _ => None,
    });
    assert_eq!(empty_wayland.kind, SessionKind::X11);
}

#[test]
fn collect_findings_is_total_on_a_live_machine() {
    // Whatever tools this machine has or lacks, collection
    // completes, every probe kind reports a summary, and the
    // findings convert to a report without panicking. This is
    // the "missing capability degrades, never crashes" law
    // exercised end to end.
    let findings = torogao_accel::collect_findings();
    assert_eq!(findings.summaries.len(), 7);
    let report = findings.into_report();
    assert!(report.adapters.len() <= torogao_accel::limits::ADAPTERS_MAX);
    assert_eq!(report.schema, "torogao-accel-capability/1");
}

#[test]
fn bounded_runner_reports_success_failure_and_missing() {
    let ok = run_bounded(&["true"], Duration::from_secs(5), 4096);
    assert_eq!(ok.status, ProbeStatus::Found);

    let failed = run_bounded(&["false"], Duration::from_secs(5), 4096);
    assert_eq!(failed.status, ProbeStatus::ToolFailed);

    let missing = run_bounded(
        &["torogao-accel-no-such-tool-7f3a"],
        Duration::from_secs(5),
        4096,
    );
    assert_eq!(missing.status, ProbeStatus::ToolMissing);
    assert_eq!(missing.stdout, None);
}

#[test]
fn bounded_runner_enforces_the_deadline() {
    let run = run_bounded(&["sleep", "30"], Duration::from_millis(150), 4096);
    assert_eq!(run.status, ProbeStatus::TimedOut);
}

#[test]
fn bounded_runner_caps_output_and_reports_truncation() {
    let run = run_bounded(
        &["head", "-c", "100000", "/dev/zero"],
        Duration::from_secs(5),
        4096,
    );
    assert_eq!(run.status, ProbeStatus::OutputTruncated);
    assert_eq!(run.stdout.map(|s| s.len()), Some(4096));
}
