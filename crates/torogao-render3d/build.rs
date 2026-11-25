//! Build script: compile the crate's GLSL shader sources to
//! SPIR-V with the system `glslc` (shaderc). Shader-language
//! ruling for rung 1 (D-1217 report): GLSL 460 + glslc, because
//! the estate's pinned Slang toolchain is not installed on primo
//! (the only `slangc` present is Nsight Graphics' bundled copy,
//! which fails to load its SPIR-V downstream compiler), while
//! glslc is present and versioned (shaderc 2026.4). Sources stay
//! in-tree under `shaders/`; the shading model lives in shared
//! `#include` chunks so the vertex, fragment and compute paths
//! evaluate one model.
//!
//! Failure mode is loud by construction: a missing `glslc` or a
//! shader compile error fails the build with the compiler's own
//! diagnostics. Never substitute precompiled blobs.

use std::path::{Path, PathBuf};
use std::process::Command;

fn compile(shader_dir: &Path, out_dir: &Path, name: &str, stage: &str) {
    let src = shader_dir.join(name);
    let out = out_dir.join(format!("{name}.spv"));
    let status = Command::new("glslc")
        .arg(format!("-fshader-stage={stage}"))
        .arg(format!("-I{}", shader_dir.display()))
        .arg("-o")
        .arg(&out)
        .arg(&src)
        .status()
        .unwrap_or_else(|e| panic!("failed to spawn glslc for {name}: {e}"));
    if !status.success() {
        panic!("glslc failed on {name} with status {status}");
    }
    println!("cargo:rerun-if-changed={}", src.display());
}

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let shader_dir = manifest.join("shaders");
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("out dir"));
    // Shared chunks: touching one recompiles every consumer.
    for chunk in ["skin.glsl", "shade.glsl", "toon.glsl"] {
        println!(
            "cargo:rerun-if-changed={}",
            shader_dir.join(chunk).display()
        );
    }
    compile(&shader_dir, &out_dir, "character.vert", "vertex");
    compile(&shader_dir, &out_dir, "character.frag", "fragment");
    compile(&shader_dir, &out_dir, "outline.vert", "vertex");
    compile(&shader_dir, &out_dir, "outline.frag", "fragment");
    compile(&shader_dir, &out_dir, "skin.comp", "compute");
    compile(&shader_dir, &out_dir, "skin.vert", "vertex");
    compile(&shader_dir, &out_dir, "set.vert", "vertex");
    compile(&shader_dir, &out_dir, "set.frag", "fragment");
    compile(&shader_dir, &out_dir, "setdepth.vert", "vertex");
    compile(&shader_dir, &out_dir, "depthonly.frag", "fragment");
    compile(&shader_dir, &out_dir, "splat.vert", "vertex");
    compile(&shader_dir, &out_dir, "splat.frag", "fragment");
}
