//! The seam's RON value tree: the interchange shape between
//! this crate's typed records and the project file.
//!
//! The variant set mirrors `torogao_author::ron::Ron` exactly,
//! variant for variant, so the author layer converts between
//! the two trees by a structural identity walk and no second
//! parser ever reads `project.ron` (the author's bounded parser
//! is the file's one reader; canonical bytes come from the
//! author's one emitter, whose floats run through
//! `torogao_format::jsonfmt::fmt_f64`).
//!
//! Anonymous records in the DESIGN §1.2 sketch (`(slot: …,
//! text: …)`) take their type's name here (`Slot(...)`), the
//! same rule the author's codec applies to its own records
//! (`Rect(...)`): the tree has no anonymous struct shape.

/// One RON value, in the seam's tree form.
#[derive(Clone, Debug, PartialEq)]
pub enum Ron {
    /// `true` / `false`.
    Bool(bool),
    /// A float, emitted through the canonical float writer.
    Float(f64),
    /// A bare identifier: a unit enum variant.
    Ident(String),
    /// An integer literal.
    Int(i64),
    /// `[a, b, ...]`.
    List(Vec<Ron>),
    /// `None` or `Some(value)`.
    Opt(Option<Box<Ron>>),
    /// A double-quoted string.
    Str(String),
    /// `Name(field: value, ...)`, fields in emission order.
    Struct(String, Vec<(String, Ron)>),
    /// `(a, b, ...)`.
    Tuple(Vec<Ron>),
}

/// A struct record being consumed field by field, in the
/// author's codec manner: unknown fields refuse at `done`,
/// missing fields refuse at `take`.
pub struct Fields {
    kind: String,
    items: Vec<(String, Ron)>,
}

/// Opens a value as the named struct record.
pub fn open(value: Ron, expected: &str) -> crate::error::GencondResult<Fields> {
    match value {
        Ron::Struct(name, items) if name == expected => Ok(Fields { kind: name, items }),
        Ron::Struct(name, _) => crate::error::refuse(
            crate::error::CODE_UNKNOWN_KIND,
            format!("record kind '{name}' is not known where '{expected}' is expected"),
        ),
        _ => crate::error::refuse(
            crate::error::CODE_MALFORMED,
            format!("expected a {expected} record"),
        ),
    }
}

impl Fields {
    /// Takes a field's value; refuses when absent.
    pub fn take(&mut self, key: &str) -> crate::error::GencondResult<Ron> {
        match self.items.iter().position(|(name, _)| name == key) {
            Some(index) => Ok(self.items.remove(index).1),
            None => crate::error::refuse(
                crate::error::CODE_MALFORMED,
                format!("{} record lacks field '{key}'", self.kind),
            ),
        }
    }

    /// Takes a field when present; `None` when absent (for the
    /// one defaulted field, `window`, DESIGN §1.3).
    pub fn try_take(&mut self, key: &str) -> Option<Ron> {
        self.items
            .iter()
            .position(|(name, _)| name == key)
            .map(|index| self.items.remove(index).1)
    }

    /// Refuses when unconsumed fields remain.
    pub fn done(self) -> crate::error::GencondResult<()> {
        match self.items.first() {
            Some((name, _)) => crate::error::refuse(
                crate::error::CODE_MALFORMED,
                format!("{} record has unknown field '{name}'", self.kind),
            ),
            None => Ok(()),
        }
    }

    /// Takes a string field.
    pub fn string(&mut self, key: &str) -> crate::error::GencondResult<String> {
        match self.take(key)? {
            Ron::Str(text) => Ok(text),
            _ => wrong_type(key, "a string"),
        }
    }

    /// Takes a float field (an integer literal widens, as the
    /// project file's floats are written by value not by kind).
    pub fn float(&mut self, key: &str) -> crate::error::GencondResult<f64> {
        match self.take(key)? {
            Ron::Float(number) => Ok(number),
            Ron::Int(number) => Ok(number as f64),
            _ => wrong_type(key, "a float"),
        }
    }

    /// Takes an integer field.
    pub fn int(&mut self, key: &str) -> crate::error::GencondResult<i64> {
        match self.take(key)? {
            Ron::Int(number) => Ok(number),
            _ => wrong_type(key, "an integer"),
        }
    }

    /// Takes a boolean field.
    pub fn boolean(&mut self, key: &str) -> crate::error::GencondResult<bool> {
        match self.take(key)? {
            Ron::Bool(flag) => Ok(flag),
            _ => wrong_type(key, "a boolean"),
        }
    }

    /// Takes a list field.
    pub fn list(&mut self, key: &str) -> crate::error::GencondResult<Vec<Ron>> {
        match self.take(key)? {
            Ron::List(items) => Ok(items),
            _ => wrong_type(key, "a list"),
        }
    }

    /// Takes a bare-identifier (unit variant) field.
    pub fn ident(&mut self, key: &str) -> crate::error::GencondResult<String> {
        match self.take(key)? {
            Ron::Ident(name) => Ok(name),
            _ => wrong_type(key, "a variant name"),
        }
    }
}

fn wrong_type<T>(key: &str, expected: &str) -> crate::error::GencondResult<T> {
    crate::error::refuse(
        crate::error::CODE_MALFORMED,
        format!("field '{key}' is not {expected}"),
    )
}

/// Builds a struct value from a name and ordered fields.
pub fn record(name: &str, fields: Vec<(&str, Ron)>) -> Ron {
    Ron::Struct(
        name.to_string(),
        fields
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect(),
    )
}
