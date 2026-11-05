//! Real Torogao operator data for the shared workload.
//!
//! The rig data is measured by `torogao_author::run_verb` with
//! `project_inspect` over a fixture project built through the
//! authoring pipeline itself. The scene data is the repository's
//! `scenes/studio.json` record, parsed as data rather than copied
//! into either renderer.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};
use torogao_author::{Refusal, RefusalClass, Status};

use crate::error::{BakeoffError, Result};

static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(1);

const FIXTURE_SCRIPT: &str = r#"
{"verb":"project_init","inputs":{"name":"bakeoff","stamp":{"basis":"test","lane":"all-ages","pack":"all-ages","rating":"all-ages"},"consent":{"adult":true,"schema":"torogao-consent/1","wants_selected_scenes":true}}}
{"verb":"param_define","inputs":{"id":"param_a","class":"angle","min":0.0,"default":0.5,"max":1.0}}
{"verb":"param_define","inputs":{"id":"param_b","class":"angle","min":-1.0,"default":0.0,"max":1.0}}
{"verb":"param_pose_default_set","inputs":{"parameter":"param_a","value":0.25}}
{"verb":"expression_define","inputs":{"id":"smile","blend":"additive","values":[{"parameter":"param_a","value":0.1}],"visibility":[]}}
"#;

/// A compact rig census extracted from the authoring answer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RigData {
    /// Project name.
    pub project_name: String,
    /// First twelve hexadecimal characters of the project hash.
    pub project_sha_prefix: String,
    /// Parameter count.
    pub parameter_count: usize,
    /// Parameter identifiers in census order.
    pub parameter_ids: Vec<String>,
    /// Part count.
    pub part_count: usize,
    /// Deformer count.
    pub deformer_count: usize,
    /// Expression count.
    pub expression_count: usize,
    /// Total mesh vertices.
    pub vertex_count: usize,
    /// Total mesh triangles.
    pub triangle_count: usize,
}

impl RigData {
    /// The census as the MCP/A2A payload shape.
    pub fn to_json(&self) -> Value {
        json!({
            "deformers": self.deformer_count,
            "expressions": self.expression_count,
            "parameters": self.parameter_count,
            "parameter_ids": self.parameter_ids,
            "parts": self.part_count,
            "project": self.project_name,
            "project_sha_prefix": self.project_sha_prefix,
            "triangles": self.triangle_count,
            "vertices": self.vertex_count,
        })
    }
}

/// One repository scene record in compact form.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SceneData {
    /// Scene identifier.
    pub name: String,
    /// Human-readable title.
    pub title: String,
    /// Scene format token.
    pub format: String,
    /// Character record count.
    pub character_count: usize,
    /// Fixture record count.
    pub fixture_count: usize,
    /// Prop record count.
    pub prop_count: usize,
}

/// The two data halves rendered by Scene and Rig.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperatorData {
    /// Rig census.
    pub rig: RigData,
    /// Scene record.
    pub scene: SceneData,
}

/// Loads the workload data from a Torogao workspace root.
///
/// # Errors
///
/// Returns a typed error when the fixture cannot be built, the
/// authoring census is not an `Ok` answer, or the scene record is
/// missing or malformed.
pub fn load_operator_data(workspace_root: &Path) -> Result<OperatorData> {
    let fixture_root = build_fixture_project()?;
    let rig = inspect_fixture_project(&fixture_root);
    let cleanup = fs::remove_dir_all(&fixture_root).map_err(|source| BakeoffError::Io {
        path: fixture_root.clone(),
        source,
    });
    let rig = rig?;
    cleanup?;
    let scene = load_scene_record(workspace_root)?;
    Ok(OperatorData { rig, scene })
}

fn build_fixture_project() -> Result<PathBuf> {
    let sequence = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "torogao-tui-bakeoff-{}-{sequence}",
        std::process::id()
    ));
    if root.exists() {
        fs::remove_dir_all(&root).map_err(|source| BakeoffError::Io {
            path: root.clone(),
            source,
        })?;
    }
    let project = root.join("bakeoff");
    fs::create_dir_all(&project).map_err(|source| BakeoffError::Io {
        path: project.clone(),
        source,
    })?;
    let requests = torogao_author::parse_script(FIXTURE_SCRIPT)
        .map_err(|refusal| BakeoffError::Author(refusal.message))?;
    let answer = torogao_author::run_requests(&project, &project, &requests);
    if answer.status != Status::Ok {
        return Err(BakeoffError::Author(format!(
            "fixture build ended {:?}: {}",
            answer.status, answer.doc
        )));
    }
    Ok(project)
}

fn inspect_fixture_project(project: &Path) -> Result<RigData> {
    let resolve = |_: &str| -> std::result::Result<PathBuf, Refusal> {
        Err(Refusal::new(
            RefusalClass::UnknownTarget,
            "the bake-off census resolves no other project",
        ))
    };
    let answer =
        torogao_author::run_verb(project, project, "project_inspect", &json!({}), &resolve);
    if answer.status != Status::Ok {
        return Err(BakeoffError::Author(format!(
            "project_inspect ended {:?}: {}",
            answer.status, answer.doc
        )));
    }
    rig_from_census(&answer.doc)
}

fn rig_from_census(document: &Value) -> Result<RigData> {
    let census = document
        .get("census")
        .ok_or_else(|| BakeoffError::Author("project_inspect omitted census".to_string()))?;
    let parameters = census
        .get("parameters")
        .ok_or_else(|| BakeoffError::Author("census omitted parameters".to_string()))?;
    let table = parameters
        .get("table")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            BakeoffError::Author("census parameter table is not an array".to_string())
        })?;
    let parameter_ids = table
        .iter()
        .filter_map(|entry| entry.get("id").and_then(Value::as_str).map(str::to_owned))
        .collect::<Vec<_>>();
    if parameter_ids.len() != table.len() {
        return Err(BakeoffError::Author(
            "census parameter table contains an entry without an id".to_string(),
        ));
    }
    let sha = document
        .get("sha256")
        .and_then(Value::as_str)
        .ok_or_else(|| BakeoffError::Author("project_inspect omitted sha256".to_string()))?;
    Ok(RigData {
        project_name: string_field(census, "project")?,
        project_sha_prefix: sha.chars().take(12).collect(),
        parameter_count: usize_field(parameters, "count")?,
        parameter_ids,
        part_count: usize_field(
            census
                .get("parts")
                .ok_or_else(|| BakeoffError::Author("census omitted parts".to_string()))?,
            "count",
        )?,
        deformer_count: usize_field(
            census
                .get("deformers")
                .ok_or_else(|| BakeoffError::Author("census omitted deformers".to_string()))?,
            "count",
        )?,
        expression_count: array_len(
            census
                .get("expressions")
                .ok_or_else(|| BakeoffError::Author("census omitted expressions".to_string()))?,
        )?,
        vertex_count: usize_field(
            census
                .get("mesh_density")
                .ok_or_else(|| BakeoffError::Author("census omitted mesh_density".to_string()))?,
            "total_vertices",
        )?,
        triangle_count: usize_field(
            census
                .get("mesh_density")
                .ok_or_else(|| BakeoffError::Author("census omitted mesh_density".to_string()))?,
            "total_triangles",
        )?,
    })
}

fn load_scene_record(workspace_root: &Path) -> Result<SceneData> {
    let path = workspace_root.join("scenes/studio.json");
    let bytes = fs::read(&path).map_err(|source| BakeoffError::Io {
        path: path.clone(),
        source,
    })?;
    if bytes.len() > 1_048_576 {
        return Err(BakeoffError::Author(format!(
            "scene record '{}' exceeds the 1048576-byte workload bound",
            path.display()
        )));
    }
    let document: Value = serde_json::from_slice(&bytes)?;
    Ok(SceneData {
        name: string_field(&document, "name")?,
        title: string_field(&document, "title")?,
        format: string_field(&document, "format")?,
        character_count: array_len_field(&document, "characters")?,
        fixture_count: array_len_field(&document, "fixtures")?,
        prop_count: array_len_field(&document, "props")?,
    })
}

fn string_field(document: &Value, field: &str) -> Result<String> {
    document
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| BakeoffError::Author(format!("record omitted string field '{field}'")))
}

fn usize_field(document: &Value, field: &str) -> Result<usize> {
    document
        .get(field)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| BakeoffError::Author(format!("record omitted bounded count '{field}'")))
}

fn array_len(document: &Value) -> Result<usize> {
    if let Some(array) = document.as_array() {
        return Ok(array.len());
    }
    if let Some(object) = document.as_object() {
        if let Some(count) = object.get("count") {
            return usize_field(&json!({"count": count}), "count");
        }
        // Other census inventory objects use a nested array.
        for key in ["defined", "expressions", "inventory", "items"] {
            if let Some(array) = object.get(key).and_then(Value::as_array) {
                return Ok(array.len());
            }
        }
        return Err(BakeoffError::Author(
            "census expression inventory has no count-bearing array".to_string(),
        ));
    }
    Err(BakeoffError::Author(
        "record field is neither an array nor an inventory object".to_string(),
    ))
}

fn array_len_field(document: &Value, field: &str) -> Result<usize> {
    let value = document
        .get(field)
        .ok_or_else(|| BakeoffError::Author(format!("record omitted field '{field}'")))?;
    value
        .as_array()
        .map(Vec::len)
        .ok_or_else(|| BakeoffError::Author(format!("record field '{field}' is not an array")))
}
