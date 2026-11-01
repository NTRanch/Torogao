//! Grammar audits: the coverage report the gates read.
//!
//! Coverage is the program's core promise — every tool,
//! verb, and mode has a spoken form, or the build says so.
//! This module computes that report from a grammar document
//! and the live sources of truth; the coverage test and the
//! `torogao-voice coverage` command both consume it.

use crate::generate;
use crate::grammar::{GrammarDoc, Target};
use serde::Serialize;
use std::collections::BTreeMap;

/// The coverage report for one grammar document.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CoverageReport {
    /// Registered tools in the live registry.
    pub tools_total: usize,
    /// Tools with a grammar entry.
    pub tools_covered: usize,
    /// Tool names lacking an entry, sorted.
    pub missing_tools: Vec<String>,
    /// Authoring verbs in the manifests (47 + 22).
    pub verbs_total: usize,
    /// Verbs with a grammar entry.
    pub verbs_covered: usize,
    /// Verb names lacking an entry, sorted.
    pub missing_verbs: Vec<String>,
    /// CLI modes in the manifest.
    pub modes_total: usize,
    /// Modes with a grammar entry.
    pub modes_covered: usize,
    /// Mode names lacking an entry, sorted.
    pub missing_modes: Vec<String>,
    /// Excluded target names found in the grammar (must be
    /// empty — see [`generate::EXCLUDED_TARGET_NAMES`]).
    pub exclusions_present: Vec<String>,
    /// Template strings claimed by more than one entry
    /// (a standing ambiguity hazard), sorted.
    pub duplicate_templates: Vec<String>,
    /// True when every check above is clean.
    pub ok: bool,
}

/// Computes the coverage report for `grammar`.
pub fn coverage_report(grammar: &GrammarDoc) -> CoverageReport {
    let tool_names: Vec<String> = torogao_cli::registry::list_tools()
        .iter()
        .map(|tool| tool.name.to_string())
        .collect();
    let covered_tools: Vec<&str> = grammar
        .entries
        .iter()
        .filter_map(|entry| match &entry.target {
            Target::RegistryTool { name } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    let missing_tools: Vec<String> = tool_names
        .iter()
        .filter(|name| !covered_tools.contains(&name.as_str()))
        .cloned()
        .collect();

    let verbs = generate::all_verbs();
    let covered_verbs: Vec<&str> = grammar
        .entries
        .iter()
        .filter_map(|entry| match &entry.target {
            Target::AuthorVerb { name } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    let missing_verbs: Vec<String> = verbs
        .iter()
        .filter(|name| !covered_verbs.contains(name))
        .map(ToString::to_string)
        .collect();

    let modes = generate::cli_modes();
    let covered_modes: Vec<&str> = grammar
        .entries
        .iter()
        .filter_map(|entry| match &entry.target {
            Target::CliMode { name } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    let missing_modes: Vec<String> = modes
        .iter()
        .filter(|name| !covered_modes.contains(name))
        .map(ToString::to_string)
        .collect();

    // Exclusions match whole target names, exactly: the
    // voice-reachable `open-purge-reset` screen is by design,
    // while a target named `purge` / `purge-execute` /
    // `haptics-arm-live` must never appear.
    let mut exclusions_present: Vec<String> = Vec::new();
    for entry in &grammar.entries {
        let target_name = match &entry.target {
            Target::RegistryTool { name }
            | Target::AuthorVerb { name }
            | Target::CliMode { name }
            | Target::AppCommand { name } => name.as_str(),
            Target::SafetyStop => "",
        };
        if generate::EXCLUDED_TARGET_NAMES.contains(&target_name) {
            exclusions_present.push(entry.id.clone());
        }
    }
    exclusions_present.sort();
    exclusions_present.dedup();

    let mut by_template: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for entry in &grammar.entries {
        for template in &entry.templates {
            by_template
                .entry(template.clone())
                .or_default()
                .push(entry.id.clone());
        }
    }
    let duplicate_templates: Vec<String> = by_template
        .iter()
        .filter(|(_, ids)| {
            let mut unique: Vec<&String> = ids.iter().collect();
            unique.sort();
            unique.dedup();
            unique.len() > 1
        })
        .map(|(template, _)| template.clone())
        .collect();

    let ok = missing_tools.is_empty()
        && missing_verbs.is_empty()
        && missing_modes.is_empty()
        && exclusions_present.is_empty()
        && duplicate_templates.is_empty();
    CoverageReport {
        tools_total: tool_names.len(),
        tools_covered: tool_names.len() - missing_tools.len(),
        missing_tools,
        verbs_total: verbs.len(),
        verbs_covered: verbs.len() - missing_verbs.len(),
        missing_verbs,
        modes_total: modes.len(),
        modes_covered: modes.len() - missing_modes.len(),
        missing_modes,
        exclusions_present,
        duplicate_templates,
        ok,
    }
}
