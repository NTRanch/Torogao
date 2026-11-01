//! Evaluator tests: (settings, capability report) -> plan.
//! The primo report is assembled from the authentic primo
//! probe fixtures, so these tests run against the real shape
//! of the reference machine: hybrid Intel iGPU + RTX 4070 +
//! RTX A6000 eGPU + llvmpipe.

use torogao_accel::probe::{
    ProbeFindings, classify_vulkan_icd, parse_clinfo_list, parse_nvidia_smi_header,
    parse_nvidia_smi_query_csv, parse_vulkaninfo_summary,
};
use torogao_accel::{
    AccelSettings, AdapterBackend, AdapterRecord, CapabilityReport, ComputeBackend, ComputeIcdPin,
    DegradationSeverity, DeviceKind, EffectiveRenderBackend, IcdRecord, PowerPref,
    RenderBackendPref, SessionKind, SessionRecord, evaluate,
};

fn primo_report() -> CapabilityReport {
    let vulkan_devices =
        parse_vulkaninfo_summary(include_str!("fixtures/vulkaninfo-summary-primo.txt"))
            .expect("vulkaninfo fixture parses");
    let header = parse_nvidia_smi_header(include_str!("fixtures/nvidia-smi-primo.txt"))
        .expect("nvidia-smi fixture parses");
    let csv = parse_nvidia_smi_query_csv(include_str!("fixtures/nvidia-smi-query-primo.txt"));
    let icd_text = include_str!("fixtures/icd-listings-primo.txt");
    let (vulkan_part, opencl_part) = icd_text.split_once("---").expect("two listings");
    let vulkan_icds = vulkan_part
        .lines()
        .filter(|line| line.ends_with(".json"))
        .map(|name| IcdRecord {
            file_name: name.to_owned(),
            vendor: classify_vulkan_icd(name),
        })
        .collect();
    let opencl_icd_files = opencl_part
        .lines()
        .filter(|line| line.ends_with(".icd"))
        .map(str::to_owned)
        .collect();
    ProbeFindings {
        vulkan_devices,
        nvidia_driver_version: Some(header.driver_version),
        cuda_version: header.cuda_version,
        cuda_devices: csv.into_iter().map(|(name, _)| name).collect(),
        opencl_platforms: parse_clinfo_list(include_str!("fixtures/clinfo-list-primo.txt")),
        gl: None,
        vulkan_icds,
        vulkan_icd_dir: Some("/usr/share/vulkan/icd.d".to_owned()),
        opencl_icd_files,
        session: SessionRecord {
            kind: SessionKind::Wayland,
            wayland_display: true,
            x11_display: true,
        },
        summaries: Vec::new(),
    }
    .into_report()
}

fn compute_on() -> AccelSettings {
    AccelSettings {
        gpu_compute_enabled: true,
        ..AccelSettings::estate_defaults()
    }
}

#[test]
fn primo_report_is_hybrid_and_carries_its_facts() {
    let report = primo_report();
    assert!(report.is_hybrid());
    assert_eq!(report.adapters.len(), 4);
    assert_eq!(
        report.nvidia_icd_path().as_deref(),
        Some("/usr/share/vulkan/icd.d/nvidia_icd.json")
    );
    let cuda = report.cuda.as_ref().expect("CUDA record present");
    assert_eq!(cuda.driver_version, "615.78.08");
    assert_eq!(cuda.cuda_umd_version.as_deref(), Some("13.4"));
    assert_eq!(cuda.devices.len(), 2);
    assert_eq!(report.opencl_platforms.len(), 5);
    assert_eq!(report.session.kind, SessionKind::Wayland);
}

#[test]
fn defaults_on_primo_render_vulkan_on_the_4070() {
    let plan = evaluate(&AccelSettings::estate_defaults(), &primo_report());
    assert_eq!(plan.render.backend, EffectiveRenderBackend::Vulkan);
    assert_eq!(
        plan.render.adapter_name.as_deref(),
        Some("NVIDIA GeForce RTX 4070 Laptop GPU")
    );
    assert!(plan.render.accelerated);
    assert!(!plan.compute.enabled);
    assert!(plan.degradations.is_empty());
}

#[test]
fn low_power_prefers_the_integrated_adapter() {
    let settings = AccelSettings {
        power_preference: PowerPref::LowPower,
        ..AccelSettings::estate_defaults()
    };
    let plan = evaluate(&settings, &primo_report());
    assert_eq!(
        plan.render.adapter_name.as_deref(),
        Some("Intel(R) Graphics (RPL-P)")
    );
}

#[test]
fn compute_on_primo_uses_vulkan_with_the_nvidia_pin() {
    let plan = evaluate(&compute_on(), &primo_report());
    assert!(plan.compute.enabled);
    assert_eq!(plan.compute.backend, Some(ComputeBackend::Vulkan));
    assert_eq!(
        plan.compute.device_name.as_deref(),
        Some("NVIDIA GeForce RTX 4070 Laptop GPU")
    );
    assert_eq!(
        plan.compute.env_pairs,
        vec![(
            "VK_ICD_FILENAMES".to_owned(),
            "/usr/share/vulkan/icd.d/nvidia_icd.json".to_owned()
        )]
    );
    assert!(
        plan.degradations
            .iter()
            .all(|d| d.severity != DegradationSeverity::FeatureOff)
    );
}

#[test]
fn system_default_on_a_hybrid_machine_is_overridden_and_recorded() {
    let settings = AccelSettings {
        compute_icd_pin: ComputeIcdPin::SystemDefault,
        ..compute_on()
    };
    let plan = evaluate(&settings, &primo_report());
    assert!(plan.compute.enabled);
    assert_eq!(plan.compute.env_pairs.len(), 1);
    assert!(plan.degradations.iter().any(|d| {
        d.code == "TG-ACCEL-NOTE-PIN-OVERRIDE" && d.severity == DegradationSeverity::Note
    }));
}

#[test]
fn master_off_is_software_rendering_and_no_compute() {
    let settings = AccelSettings {
        master_enabled: false,
        ..AccelSettings::estate_defaults()
    };
    let plan = evaluate(&settings, &primo_report());
    assert_eq!(plan.render.backend, EffectiveRenderBackend::Software);
    assert!(!plan.render.accelerated);
    assert_eq!(
        plan.render.adapter_name.as_deref(),
        Some("llvmpipe (LLVM 23.1.3, 256 bits)")
    );
    assert!(!plan.compute.enabled);
    assert!(plan.degradations.is_empty());
}

#[test]
fn missing_nvidia_icd_falls_back_to_cuda_with_a_note() {
    let mut report = primo_report();
    report
        .vulkan_icds
        .retain(|icd| icd.file_name != "nvidia_icd.json");
    let settings = AccelSettings {
        compute_icd_pin: ComputeIcdPin::NvidiaOnly,
        ..compute_on()
    };
    let plan = evaluate(&settings, &report);
    assert!(plan.compute.enabled);
    assert_eq!(plan.compute.backend, Some(ComputeBackend::Cuda));
    assert!(
        plan.degradations
            .iter()
            .any(|d| d.code == "TG-ACCEL-NOTE-COMPUTE-FALLBACK")
    );
}

#[test]
fn missing_icd_and_cuda_falls_back_to_opencl() {
    let mut report = primo_report();
    report
        .vulkan_icds
        .retain(|icd| icd.file_name != "nvidia_icd.json");
    report.cuda = None;
    let settings = AccelSettings {
        compute_icd_pin: ComputeIcdPin::NvidiaOnly,
        ..compute_on()
    };
    let plan = evaluate(&settings, &report);
    assert!(plan.compute.enabled);
    assert_eq!(plan.compute.backend, Some(ComputeBackend::OpenCl));
}

#[test]
fn empty_report_degrades_everything_with_reasons() {
    let plan = evaluate(&compute_on(), &CapabilityReport::empty());
    assert_eq!(plan.render.backend, EffectiveRenderBackend::Software);
    assert!(!plan.compute.enabled);
    assert!(
        plan.degradations
            .iter()
            .any(|d| d.code == "TG-ACCEL-DEG-NO-GPU-ADAPTER")
    );
    assert!(plan.degradations.iter().any(|d| {
        d.code == "TG-ACCEL-DEG-COMPUTE-UNAVAILABLE"
            && d.severity == DegradationSeverity::FeatureOff
    }));
}

#[test]
fn explicit_vulkan_falls_back_to_opengl_when_only_gl_exists() {
    let mut report = CapabilityReport::empty();
    report.adapters.push(AdapterRecord {
        name: "Old Faithful GL".to_owned(),
        backend: AdapterBackend::OpenGl,
        device_kind: DeviceKind::DiscreteGpu,
        driver_name: "Mesa".to_owned(),
        driver_info: "Mesa 26.2.5".to_owned(),
        software: false,
    });
    let settings = AccelSettings {
        render_backend: RenderBackendPref::Vulkan,
        ..AccelSettings::estate_defaults()
    };
    let plan = evaluate(&settings, &report);
    assert_eq!(plan.render.backend, EffectiveRenderBackend::OpenGl);
    assert!(
        plan.degradations
            .iter()
            .any(|d| d.code == "TG-ACCEL-DEG-BACKEND-FALLBACK")
    );
}

#[test]
fn explicit_software_preference_is_not_a_degradation() {
    let settings = AccelSettings {
        render_backend: RenderBackendPref::Software,
        ..AccelSettings::estate_defaults()
    };
    let plan = evaluate(&settings, &primo_report());
    assert_eq!(plan.render.backend, EffectiveRenderBackend::Software);
    assert!(plan.degradations.is_empty());
}

#[test]
fn primo_report_and_plan_serialize_round_trip() {
    let report = primo_report();
    let json = serde_json::to_string(&report).expect("report serializes");
    let back: CapabilityReport = serde_json::from_str(&json).expect("report parses");
    assert_eq!(back, report);

    let plan = evaluate(&compute_on(), &report);
    let json = serde_json::to_string(&plan).expect("plan serializes");
    let back: torogao_accel::EffectivePlan = serde_json::from_str(&json).expect("plan parses");
    assert_eq!(back, plan);
}
