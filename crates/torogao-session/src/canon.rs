//! Canonical RON emission and parsing for editor-state
//! records, plus the state hash. The emitter's output is the
//! AUTH-D1-form record text; the hand-written parser is the
//! only load path for emitted records (replay never touches
//! session-seed JSON). Carried over from the Slice-1 harness
//! unchanged.

use crate::refusal::{R, Refusal};
use crate::state::{
    Binding, EditorState, Node, NodeKind, ParamState, ProjectMeta, STATE_FORMAT, Scene, Transform,
    Viewport,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// Canonical f32: `-0.0` normalises to `0.0`; shortest round-trip decimal
/// (Rust `Display` for f32 is exactly that); a bare integer gains `.0` so
/// the emitted text is unambiguously a float.
pub fn fmt_f32(v: f32) -> String {
    assert!(v.is_finite(), "canonical form has no NaN/infinity");
    if v == 0.0 {
        return "0.0".to_string();
    }
    let s = format!("{v}");
    if s.contains('.') || s.contains('e') || s.contains('E') {
        s
    } else {
        format!("{s}.0")
    }
}

fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{{{:x}}}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The canonical RON emission of a state — the AUTH-D1-form record text.
/// The viewport is never recorded (DESIGN §4.2). Field order is the schema
/// declaration order; map keys are byte-sorted (BTreeMap); nodes are
/// emitted in ascending id order; selection in ascending id order;
/// bindings sorted by (param, node, property).
pub fn to_canon_ron(st: &EditorState) -> String {
    let mut o = String::new();
    o.push_str("(\n");
    o.push_str(&format!("    format: {},\n", esc(STATE_FORMAT)));
    o.push_str(&format!(
        "    project: (name: {}, source: {}),\n",
        esc(&st.project.name),
        esc(&st.project.source)
    ));
    o.push_str(&format!(
        "    scene: (name: {}, nodes: [\n",
        esc(&st.scene.name)
    ));
    let mut nodes: Vec<&Node> = st.scene.nodes.iter().collect();
    nodes.sort_by_key(|n| n.id);
    for n in nodes {
        let parent = match n.parent {
            Some(p) => format!("Some({p})"),
            None => "None".to_string(),
        };
        let kind = match n.kind {
            NodeKind::Node => "Node",
            NodeKind::Part => "Part",
            NodeKind::SimplePhysics => "SimplePhysics",
        };
        let t = n.transform;
        o.push_str(&format!(
            "        (id: {}, name: {}, parent: {}, kind: {}, visible: {}, zsort: {}, transform: (tx: {}, ty: {}, rot: {}, sx: {}, sy: {})),\n",
            n.id,
            esc(&n.name),
            parent,
            kind,
            n.visible,
            fmt_f32(n.zsort),
            fmt_f32(t.tx),
            fmt_f32(t.ty),
            fmt_f32(t.rot),
            fmt_f32(t.sx),
            fmt_f32(t.sy),
        ));
    }
    o.push_str("    ]),\n");
    let mut sel = st.selection.clone();
    sel.sort_unstable();
    let sel_s: Vec<String> = sel.iter().map(|i| i.to_string()).collect();
    o.push_str(&format!("    selection: [{}],\n", sel_s.join(", ")));
    o.push_str("    params: {\n");
    for (name, p) in &st.params {
        o.push_str(&format!(
            "        {}: (value: {}, min: {}, max: {}),\n",
            esc(name),
            fmt_f32(p.value),
            fmt_f32(p.min),
            fmt_f32(p.max)
        ));
    }
    o.push_str("    },\n");
    let mut bindings: Vec<&Binding> = st.bindings.iter().collect();
    bindings.sort_by(|a, b| (&a.param, a.node, &a.property).cmp(&(&b.param, b.node, &b.property)));
    o.push_str("    bindings: [\n");
    for b in bindings {
        o.push_str(&format!(
            "        (param: {}, node: {}, property: {}, kind: {}),\n",
            esc(&b.param),
            b.node,
            esc(&b.property),
            esc(&b.kind)
        ));
    }
    o.push_str("    ],\n");
    let unc: Vec<String> = st.uncapturable.iter().map(|u| esc(u)).collect();
    o.push_str(&format!("    uncapturable: [{}],\n", unc.join(", ")));
    o.push_str(")\n");
    o
}

/// The state hash: SHA-256 (lowercase hex) over the canonical RON bytes.
pub fn state_hash(st: &EditorState) -> String {
    let mut h = Sha256::new();
    h.update(to_canon_ron(st).as_bytes());
    format!("{:x}", h.finalize())
}
// structs `(field: value, ...)`, lists `[...]`, maps `{ "k": value, ... }`,
// strings, numbers, `true`/`false`, idents, `Some(v)` / `None`.
// This parser is the *only* load path for emitted records: replay never
// touches the session seed JSON.

#[derive(Debug, Clone, PartialEq)]
enum Value {
    Str(String),
    Num(f32),
    Int(i64),
    Bool(bool),
    Ident(String),
    Some(Box<Value>),
    None,
    List(Vec<Value>),
    Struct(Vec<(String, Value)>),
    Map(Vec<(String, Value)>),
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Parser<'a> {
    fn err(&self, msg: &str) -> Refusal {
        Refusal::new(
            "TG-SES-SCHEMA",
            format!("RON parse at byte {}: {msg}", self.i),
        )
    }

    fn skip_ws(&mut self) {
        loop {
            while self.i < self.b.len() && (self.b[self.i] as char).is_whitespace() {
                self.i += 1;
            }
            if self.i + 1 < self.b.len() && &self.b[self.i..self.i + 2] == b"//" {
                while self.i < self.b.len() && self.b[self.i] != b'\n' {
                    self.i += 1;
                }
            } else {
                break;
            }
        }
    }

    fn peek(&mut self) -> R<u8> {
        self.skip_ws();
        self.b
            .get(self.i)
            .copied()
            .ok_or_else(|| self.err("unexpected end of input"))
    }

    fn expect(&mut self, c: u8) -> R<()> {
        let got = self.peek()?;
        if got == c {
            self.i += 1;
            Ok(())
        } else {
            Err(self.err(&format!("expected {:?}, got {:?}", c as char, got as char)))
        }
    }

    fn ident(&mut self) -> R<String> {
        self.skip_ws();
        let start = self.i;
        while self.i < self.b.len()
            && (self.b[self.i].is_ascii_alphanumeric() || self.b[self.i] == b'_')
        {
            self.i += 1;
        }
        if start == self.i {
            return Err(self.err("expected identifier"));
        }
        Ok(String::from_utf8_lossy(&self.b[start..self.i]).into_owned())
    }

    fn string(&mut self) -> R<String> {
        self.expect(b'"')?;
        let mut out = String::new();
        loop {
            let c = *self
                .b
                .get(self.i)
                .ok_or_else(|| self.err("unterminated string"))?;
            self.i += 1;
            match c {
                b'"' => return Ok(out),
                b'\\' => {
                    let e = *self.b.get(self.i).ok_or_else(|| self.err("bad escape"))?;
                    self.i += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            self.expect(b'{')?;
                            let start = self.i;
                            while self.i < self.b.len() && self.b[self.i] != b'}' {
                                self.i += 1;
                            }
                            let hex = String::from_utf8_lossy(&self.b[start..self.i]).into_owned();
                            self.expect(b'}')?;
                            let cp = u32::from_str_radix(&hex, 16)
                                .map_err(|_| self.err("bad \\u escape"))?;
                            out.push(char::from_u32(cp).ok_or_else(|| self.err("bad codepoint"))?);
                        }
                        _ => return Err(self.err("unknown escape")),
                    }
                }
                _ => {
                    // Reassemble UTF-8: consume the full char.
                    let mut buf = vec![c];
                    if c >= 0x80 {
                        let need = if c >= 0xF0 {
                            3
                        } else if c >= 0xE0 {
                            2
                        } else {
                            1
                        };
                        for _ in 0..need {
                            buf.push(*self.b.get(self.i).ok_or_else(|| self.err("bad utf8"))?);
                            self.i += 1;
                        }
                    }
                    out.push_str(&String::from_utf8_lossy(&buf));
                }
            }
        }
    }

    fn number(&mut self) -> R<Value> {
        self.skip_ws();
        let start = self.i;
        if self.i < self.b.len() && self.b[self.i] == b'-' {
            self.i += 1;
        }
        let mut is_float = false;
        while self.i < self.b.len() {
            match self.b[self.i] {
                b'0'..=b'9' => self.i += 1,
                b'.' => {
                    is_float = true;
                    self.i += 1;
                }
                b'e' | b'E' => {
                    is_float = true;
                    self.i += 1;
                    if self.i < self.b.len() && (self.b[self.i] == b'-' || self.b[self.i] == b'+') {
                        self.i += 1;
                    }
                }
                _ => break,
            }
        }
        let text = String::from_utf8_lossy(&self.b[start..self.i]).into_owned();
        if is_float {
            let v: f32 = text.parse().map_err(|_| self.err("bad float"))?;
            if !v.is_finite() {
                return Err(Refusal::new(
                    "TG-SES-NONFINITE",
                    format!("RON number {text} not finite"),
                ));
            }
            Ok(Value::Num(v))
        } else {
            let v: i64 = text.parse().map_err(|_| self.err("bad int"))?;
            Ok(Value::Int(v))
        }
    }

    fn value(&mut self) -> R<Value> {
        match self.peek()? {
            b'(' => {
                self.i += 1;
                let mut fields = Vec::new();
                if self.peek()? == b')' {
                    self.i += 1;
                    return Ok(Value::Struct(fields));
                }
                loop {
                    let name = self.ident()?;
                    self.expect(b':')?;
                    let v = self.value()?;
                    fields.push((name, v));
                    match self.peek()? {
                        b',' => {
                            self.i += 1;
                            if self.peek()? == b')' {
                                self.i += 1;
                                break;
                            }
                        }
                        b')' => {
                            self.i += 1;
                            break;
                        }
                        _ => return Err(self.err("expected , or ) in struct")),
                    }
                }
                Ok(Value::Struct(fields))
            }
            b'[' => {
                self.i += 1;
                let mut items = Vec::new();
                if self.peek()? == b']' {
                    self.i += 1;
                    return Ok(Value::List(items));
                }
                loop {
                    items.push(self.value()?);
                    match self.peek()? {
                        b',' => {
                            self.i += 1;
                            if self.peek()? == b']' {
                                self.i += 1;
                                break;
                            }
                        }
                        b']' => {
                            self.i += 1;
                            break;
                        }
                        _ => return Err(self.err("expected , or ] in list")),
                    }
                }
                Ok(Value::List(items))
            }
            b'{' => {
                self.i += 1;
                let mut pairs = Vec::new();
                if self.peek()? == b'}' {
                    self.i += 1;
                    return Ok(Value::Map(pairs));
                }
                loop {
                    let k = self.string()?;
                    self.expect(b':')?;
                    let v = self.value()?;
                    pairs.push((k, v));
                    match self.peek()? {
                        b',' => {
                            self.i += 1;
                            if self.peek()? == b'}' {
                                self.i += 1;
                                break;
                            }
                        }
                        b'}' => {
                            self.i += 1;
                            break;
                        }
                        _ => return Err(self.err("expected , or } in map")),
                    }
                }
                Ok(Value::Map(pairs))
            }
            b'"' => Ok(Value::Str(self.string()?)),
            b'-' | b'0'..=b'9' => self.number(),
            _ => {
                let id = self.ident()?;
                match id.as_str() {
                    "true" => Ok(Value::Bool(true)),
                    "false" => Ok(Value::Bool(false)),
                    "None" => Ok(Value::None),
                    "Some" => {
                        self.expect(b'(')?;
                        let v = self.value()?;
                        self.expect(b')')?;
                        Ok(Value::Some(Box::new(v)))
                    }
                    _ => Ok(Value::Ident(id)),
                }
            }
        }
    }
}

fn fields(v: &Value) -> R<&Vec<(String, Value)>> {
    match v {
        Value::Struct(f) => Ok(f),
        _ => Err(Refusal::new("TG-SES-SCHEMA", "expected RON struct")),
    }
}

fn get<'a>(f: &'a [(String, Value)], name: &str) -> R<&'a Value> {
    f.iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v)
        .ok_or_else(|| Refusal::new("TG-SES-SCHEMA", format!("missing field {name}")))
}

fn strict_fields(f: &[(String, Value)], allowed: &[&str]) -> R<()> {
    for (k, _) in f {
        if !allowed.contains(&k.as_str()) {
            return Err(Refusal::new(
                "TG-SES-UNKNOWN-FIELD",
                format!("record field {k:?} is not in the schema"),
            ));
        }
    }
    Ok(())
}

fn as_str(v: &Value) -> R<String> {
    match v {
        Value::Str(s) => Ok(s.clone()),
        _ => Err(Refusal::new("TG-SES-SCHEMA", "expected string")),
    }
}

fn as_f32(v: &Value) -> R<f32> {
    match v {
        Value::Num(x) => Ok(*x),
        Value::Int(i) => Ok(*i as f32),
        _ => Err(Refusal::new("TG-SES-SCHEMA", "expected number")),
    }
}

fn as_u32(v: &Value) -> R<u32> {
    match v {
        Value::Int(i) if *i >= 0 && *i <= u32::MAX as i64 => Ok(*i as u32),
        _ => Err(Refusal::new("TG-SES-SCHEMA", "expected unsigned int")),
    }
}

fn as_bool(v: &Value) -> R<bool> {
    match v {
        Value::Bool(b) => Ok(*b),
        _ => Err(Refusal::new("TG-SES-SCHEMA", "expected bool")),
    }
}

fn as_list(v: &Value) -> R<&Vec<Value>> {
    match v {
        Value::List(l) => Ok(l),
        _ => Err(Refusal::new("TG-SES-SCHEMA", "expected list")),
    }
}

/// Parse an emitted canonical RON record back into an `EditorState`.
/// The viewport is not recorded; it decodes to the session default.
pub fn from_canon_ron(text: &str) -> R<EditorState> {
    let mut p = Parser {
        b: text.as_bytes(),
        i: 0,
    };
    let v = p.value()?;
    p.skip_ws();
    if p.i != p.b.len() {
        return Err(p.err("trailing content after record"));
    }
    let f = fields(&v)?;
    strict_fields(
        f,
        &[
            "format",
            "project",
            "scene",
            "selection",
            "params",
            "bindings",
            "uncapturable",
        ],
    )?;
    let pf = fields(get(f, "project")?)?;
    strict_fields(pf, &["name", "source"])?;
    let sf = fields(get(f, "scene")?)?;
    strict_fields(sf, &["name", "nodes"])?;
    let mut nodes = Vec::new();
    for nv in as_list(get(sf, "nodes")?)? {
        let nf = fields(nv)?;
        strict_fields(
            nf,
            &[
                "id",
                "name",
                "parent",
                "kind",
                "visible",
                "zsort",
                "transform",
            ],
        )?;
        let parent = match get(nf, "parent")? {
            Value::None => None,
            Value::Some(inner) => Some(as_u32(inner)?),
            _ => {
                return Err(Refusal::new(
                    "TG-SES-SCHEMA",
                    "parent must be None or Some(id)",
                ));
            }
        };
        let kind = match get(nf, "kind")? {
            Value::Ident(s) if s == "Node" => NodeKind::Node,
            Value::Ident(s) if s == "Part" => NodeKind::Part,
            Value::Ident(s) if s == "SimplePhysics" => NodeKind::SimplePhysics,
            _ => return Err(Refusal::new("TG-SES-SCHEMA", "unknown node kind")),
        };
        let tf = fields(get(nf, "transform")?)?;
        strict_fields(tf, &["tx", "ty", "rot", "sx", "sy"])?;
        nodes.push(Node {
            id: as_u32(get(nf, "id")?)?,
            name: as_str(get(nf, "name")?)?,
            parent,
            kind,
            visible: as_bool(get(nf, "visible")?)?,
            zsort: as_f32(get(nf, "zsort")?)?,
            transform: Transform {
                tx: as_f32(get(tf, "tx")?)?,
                ty: as_f32(get(tf, "ty")?)?,
                rot: as_f32(get(tf, "rot")?)?,
                sx: as_f32(get(tf, "sx")?)?,
                sy: as_f32(get(tf, "sy")?)?,
            },
        });
    }
    let mut selection = Vec::new();
    for sv in as_list(get(f, "selection")?)? {
        selection.push(as_u32(sv)?);
    }
    let mut params = BTreeMap::new();
    match get(f, "params")? {
        Value::Map(pairs) => {
            for (k, pv) in pairs {
                let pf2 = fields(pv)?;
                strict_fields(pf2, &["value", "min", "max"])?;
                params.insert(
                    k.clone(),
                    ParamState {
                        value: as_f32(get(pf2, "value")?)?,
                        min: as_f32(get(pf2, "min")?)?,
                        max: as_f32(get(pf2, "max")?)?,
                    },
                );
            }
        }
        _ => return Err(Refusal::new("TG-SES-SCHEMA", "params must be a map")),
    }
    let mut bindings = Vec::new();
    for bv in as_list(get(f, "bindings")?)? {
        let bf = fields(bv)?;
        strict_fields(bf, &["param", "node", "property", "kind"])?;
        bindings.push(Binding {
            param: as_str(get(bf, "param")?)?,
            node: as_u32(get(bf, "node")?)?,
            property: as_str(get(bf, "property")?)?,
            kind: as_str(get(bf, "kind")?)?,
        });
    }
    let mut uncapturable = Vec::new();
    for uv in as_list(get(f, "uncapturable")?)? {
        uncapturable.push(as_str(uv)?);
    }
    let st = EditorState {
        format: as_str(get(f, "format")?)?,
        project: ProjectMeta {
            name: as_str(get(pf, "name")?)?,
            source: as_str(get(pf, "source")?)?,
        },
        scene: Scene {
            name: as_str(get(sf, "name")?)?,
            nodes,
        },
        selection,
        viewport: Viewport {
            cam_x: 0.0,
            cam_y: 0.0,
            zoom: 1.0,
        },
        params,
        bindings,
        uncapturable,
    };
    st.validate()?;
    Ok(st)
}
