//! Record census: structural counts of an `.inx` file as JSON,
//! for parity tables against the evaluator of record's census.
//!
//! Usage: rig_census --inx FILE

use std::process::ExitCode;

use torogao_rig::load_inp;
use torogao_rig::model::NodeKind;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut inx: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--inx" {
            i += 1;
            inx = args.get(i).cloned();
        }
        i += 1;
    }
    let Some(inx) = inx else {
        eprintln!("usage: rig_census --inx FILE");
        return ExitCode::from(2);
    };
    let bytes = match std::fs::read(&inx) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("rig_census: cannot read {inx}: {e}");
            return ExitCode::from(2);
        }
    };
    let model = match load_inp(&bytes) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("rig_census: import failed: {e}");
            return ExitCode::from(2);
        }
    };
    let census = model.census();
    let composite_mask_sources = model
        .nodes
        .iter()
        .filter_map(|n| n.drawable())
        .flat_map(|d| d.masks.iter())
        .filter(|m| matches!(model.nodes[m.source].kind, NodeKind::Composite(_)))
        .count();
    let report = serde_json::json!({
        "file": inx,
        "name": model.name,
        "version": model.version,
        "nodes": census.nodes(),
        "groups": census.groups,
        "mesh_groups": census.mesh_groups,
        "parts": census.parts,
        "composites": census.composites,
        "physics": census.physics,
        "masks": census.masks,
        "composite_mask_sources": composite_mask_sources,
        "params": census.params,
        "vec2_params": census.vec2_params,
        "bindings": census.bindings,
        "textures": census.textures,
        "extensions": model.extensions,
        "dropped_texture_slots": model.dropped_texture_slots,
        "dropped_unsupported_bindings": model.dropped_unsupported_bindings,
        "skipped_bindings": model.skipped_bindings,
        "tga_rle_line_crossings": model.tga_rle_line_crossings,
        "param_names": model.params.iter().map(|p| p.name.clone()).collect::<Vec<_>>(),
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&report).unwrap_or_default()
    );
    ExitCode::SUCCESS
}
