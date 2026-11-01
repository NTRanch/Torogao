//! The canonical RON layer: a value tree, its one emitter, and a
//! bounded parser for the RON subset the project file uses.
//!
//! Why in-crate rather than the `ron` crate: the brief requires
//! project floats to print through the same canonical float writer
//! `torogao-format` uses (`jsonfmt::fmt_f64`, C `%.18g`), which no
//! external RON serializer offers. One emitter here is what makes
//! byte stability a property: `emit(parse(emit(v))) == emit(v)`.
//!
//! Layout (the canonical form): named structs are multi-line with
//! four-space indentation and trailing commas; lists of scalars or
//! scalar tuples are single-line; everything else is one element
//! per line. The parser also accepts `//` and `/* */` comments and
//! any whitespace, so hand edits load; whether they are canonical
//! is decided by re-emitting and comparing bytes.

use crate::limits::RON_DEPTH_MAX;
use crate::refusal::{Refusal, RefusalClass, refuse};
use torogao_format::jsonfmt::fmt_f64;

/// One RON value.
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

const INDENT: &str = "    ";

/// Emits a value as the canonical document text: the value at
/// indentation zero plus one trailing newline.
pub fn emit_document(value: &Ron) -> String {
    let mut out = String::new();
    emit(value, 0, &mut out);
    out.push('\n');
    out
}

fn is_inline(value: &Ron) -> bool {
    match value {
        Ron::Bool(_) | Ron::Float(_) | Ron::Ident(_) | Ron::Int(_) | Ron::Str(_) => true,
        Ron::Tuple(items) => items.iter().all(is_inline),
        Ron::Opt(None) => true,
        Ron::List(_) | Ron::Opt(Some(_)) | Ron::Struct(..) => false,
    }
}

fn push_indent(level: usize, out: &mut String) {
    for _ in 0..level {
        out.push_str(INDENT);
    }
}

fn emit(value: &Ron, level: usize, out: &mut String) {
    match value {
        Ron::Bool(flag) => out.push_str(if *flag { "true" } else { "false" }),
        Ron::Float(number) => out.push_str(&fmt_f64(*number)),
        Ron::Ident(name) => out.push_str(name),
        Ron::Int(number) => out.push_str(&number.to_string()),
        Ron::Str(text) => emit_string(text, out),
        Ron::Opt(None) => out.push_str("None"),
        Ron::Opt(Some(inner)) => {
            out.push_str("Some(");
            emit(inner, level, out);
            out.push(')');
        }
        Ron::Tuple(items) => {
            out.push('(');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                emit(item, level, out);
            }
            out.push(')');
        }
        Ron::List(items) => emit_list(items, level, out),
        Ron::Struct(name, fields) => {
            out.push_str(name);
            out.push('(');
            if fields.is_empty() {
                out.push(')');
                return;
            }
            out.push('\n');
            for (field, item) in fields {
                push_indent(level + 1, out);
                out.push_str(field);
                out.push_str(": ");
                emit(item, level + 1, out);
                out.push_str(",\n");
            }
            push_indent(level, out);
            out.push(')');
        }
    }
}

fn emit_list(items: &[Ron], level: usize, out: &mut String) {
    if items.is_empty() {
        out.push_str("[]");
        return;
    }
    if items.iter().all(is_inline) {
        out.push('[');
        for (index, item) in items.iter().enumerate() {
            if index > 0 {
                out.push_str(", ");
            }
            emit(item, level, out);
        }
        out.push(']');
        return;
    }
    out.push_str("[\n");
    for item in items {
        push_indent(level + 1, out);
        emit(item, level + 1, out);
        out.push_str(",\n");
    }
    push_indent(level, out);
    out.push(']');
}

fn emit_string(text: &str, out: &mut String) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 || ch == '\u{7f}' => {
                out.push_str(&format!("\\u{{{:x}}}", ch as u32));
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
}

/// Parses one RON document: exactly one value, then only
/// whitespace or comments. Nesting deeper than `RON_DEPTH_MAX`
/// is refused; work is linear in the input length.
pub fn parse_document(text: &str) -> Result<Ron, Refusal> {
    let mut parser = Parser {
        bytes: text.as_bytes(),
        text,
        at: 0,
    };
    let value = parser.value(0)?;
    parser.skip_trivia()?;
    if parser.at != parser.bytes.len() {
        return parser.fail("trailing content after the document value");
    }
    Ok(value)
}

struct Parser<'a> {
    bytes: &'a [u8],
    text: &'a str,
    at: usize,
}

impl Parser<'_> {
    fn fail<T>(&self, what: &str) -> Result<T, Refusal> {
        let line = self.bytes[..self.at.min(self.bytes.len())]
            .iter()
            .filter(|byte| **byte == b'\n')
            .count()
            + 1;
        refuse(
            RefusalClass::Malformed,
            format!("project file syntax error at line {line}: {what}"),
        )
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn skip_trivia(&mut self) -> Result<(), Refusal> {
        loop {
            match self.peek() {
                Some(b' ' | b'\t' | b'\n' | b'\r') => self.at += 1,
                Some(b'/') if self.bytes.get(self.at + 1) == Some(&b'/') => {
                    while let Some(byte) = self.peek() {
                        if byte == b'\n' {
                            break;
                        }
                        self.at += 1;
                    }
                }
                Some(b'/') if self.bytes.get(self.at + 1) == Some(&b'*') => {
                    let rest = &self.text[self.at + 2..];
                    match rest.find("*/") {
                        Some(offset) => self.at += 2 + offset + 2,
                        None => return self.fail("unterminated block comment"),
                    }
                }
                _ => return Ok(()),
            }
        }
    }

    fn expect(&mut self, byte: u8) -> Result<(), Refusal> {
        self.skip_trivia()?;
        if self.peek() == Some(byte) {
            self.at += 1;
            Ok(())
        } else {
            self.fail(&format!("expected '{}'", byte as char))
        }
    }

    /// After an element: consumes a `,` (returning false) or sees
    /// the closing byte (consuming it, returning true).
    fn separator(&mut self, close: u8) -> Result<bool, Refusal> {
        self.skip_trivia()?;
        match self.peek() {
            Some(b',') => {
                self.at += 1;
                self.skip_trivia()?;
                if self.peek() == Some(close) {
                    self.at += 1;
                    return Ok(true);
                }
                Ok(false)
            }
            Some(byte) if byte == close => {
                self.at += 1;
                Ok(true)
            }
            _ => self.fail(&format!("expected ',' or '{}'", close as char)),
        }
    }

    fn value(&mut self, depth: usize) -> Result<Ron, Refusal> {
        if depth > RON_DEPTH_MAX {
            return self.fail("nesting exceeds the depth bound");
        }
        self.skip_trivia()?;
        match self.peek() {
            Some(b'"') => self.string().map(Ron::Str),
            Some(b'[') => self.list(depth),
            Some(b'(') => self.tuple(depth),
            Some(byte) if byte == b'-' || byte.is_ascii_digit() => self.number(),
            Some(byte) if byte.is_ascii_alphabetic() || byte == b'_' => self.named(depth),
            Some(_) => self.fail("unexpected character"),
            None => self.fail("unexpected end of document"),
        }
    }

    fn list(&mut self, depth: usize) -> Result<Ron, Refusal> {
        self.at += 1;
        let mut items = Vec::new();
        self.skip_trivia()?;
        if self.peek() == Some(b']') {
            self.at += 1;
            return Ok(Ron::List(items));
        }
        loop {
            items.push(self.value(depth + 1)?);
            if self.separator(b']')? {
                return Ok(Ron::List(items));
            }
        }
    }

    fn tuple(&mut self, depth: usize) -> Result<Ron, Refusal> {
        self.at += 1;
        let mut items = Vec::new();
        self.skip_trivia()?;
        if self.peek() == Some(b')') {
            self.at += 1;
            return Ok(Ron::Tuple(items));
        }
        loop {
            items.push(self.value(depth + 1)?);
            if self.separator(b')')? {
                return Ok(Ron::Tuple(items));
            }
        }
    }

    fn ident(&mut self) -> &str {
        let start = self.at;
        while let Some(byte) = self.peek() {
            if byte.is_ascii_alphanumeric() || byte == b'_' {
                self.at += 1;
            } else {
                break;
            }
        }
        &self.text[start..self.at]
    }

    fn named(&mut self, depth: usize) -> Result<Ron, Refusal> {
        let name = self.ident().to_string();
        match name.as_str() {
            "true" => return Ok(Ron::Bool(true)),
            "false" => return Ok(Ron::Bool(false)),
            "None" => return Ok(Ron::Opt(None)),
            _ => {}
        }
        self.skip_trivia()?;
        if self.peek() != Some(b'(') {
            if name == "Some" {
                return self.fail("'Some' needs a parenthesized value");
            }
            return Ok(Ron::Ident(name));
        }
        if name == "Some" {
            self.at += 1;
            let inner = self.value(depth + 1)?;
            self.expect(b')')?;
            return Ok(Ron::Opt(Some(Box::new(inner))));
        }
        self.at += 1;
        let fields = self.fields(depth)?;
        Ok(Ron::Struct(name, fields))
    }

    fn fields(&mut self, depth: usize) -> Result<Vec<(String, Ron)>, Refusal> {
        let mut fields: Vec<(String, Ron)> = Vec::new();
        self.skip_trivia()?;
        if self.peek() == Some(b')') {
            self.at += 1;
            return Ok(fields);
        }
        loop {
            self.skip_trivia()?;
            match self.peek() {
                Some(byte) if byte.is_ascii_alphabetic() || byte == b'_' => {}
                _ => return self.fail("expected a field name"),
            }
            let key = self.ident().to_string();
            if fields.iter().any(|(existing, _)| *existing == key) {
                return self.fail(&format!("duplicate field '{key}'"));
            }
            self.expect(b':')?;
            let item = self.value(depth + 1)?;
            fields.push((key, item));
            if self.separator(b')')? {
                return Ok(fields);
            }
        }
    }

    fn number(&mut self) -> Result<Ron, Refusal> {
        let start = self.at;
        while let Some(byte) = self.peek() {
            if byte.is_ascii_digit() || matches!(byte, b'-' | b'+' | b'.' | b'e' | b'E') {
                self.at += 1;
            } else {
                break;
            }
        }
        let token = &self.text[start..self.at];
        let is_float = token.contains(['.', 'e', 'E']);
        if is_float {
            match token.parse::<f64>() {
                Ok(number) if number.is_finite() => Ok(Ron::Float(number)),
                _ => self.fail(&format!("invalid float '{token}'")),
            }
        } else {
            match token.parse::<i64>() {
                Ok(number) => Ok(Ron::Int(number)),
                Err(_) => self.fail(&format!("invalid integer '{token}'")),
            }
        }
    }

    fn string(&mut self) -> Result<String, Refusal> {
        self.at += 1;
        let mut out = String::new();
        loop {
            let Some(ch) = self.text[self.at..].chars().next() else {
                return self.fail("unterminated string");
            };
            self.at += ch.len_utf8();
            match ch {
                '"' => return Ok(out),
                '\\' => out.push(self.escape()?),
                ch => out.push(ch),
            }
        }
    }

    fn escape(&mut self) -> Result<char, Refusal> {
        let Some(byte) = self.peek() else {
            return self.fail("unterminated escape");
        };
        self.at += 1;
        match byte {
            b'"' => Ok('"'),
            b'\\' => Ok('\\'),
            b'n' => Ok('\n'),
            b'r' => Ok('\r'),
            b't' => Ok('\t'),
            b'u' => {
                self.expect(b'{')?;
                let start = self.at;
                while self.peek().is_some_and(|b| b.is_ascii_hexdigit()) && self.at - start < 6 {
                    self.at += 1;
                }
                let digits = &self.text[start..self.at];
                self.expect(b'}')?;
                match u32::from_str_radix(digits, 16)
                    .ok()
                    .and_then(char::from_u32)
                {
                    Some(ch) => Ok(ch),
                    None => self.fail("invalid unicode escape"),
                }
            }
            _ => self.fail("unknown string escape"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Ron, emit_document, parse_document};

    fn sample() -> Ron {
        Ron::Struct(
            "Doc".to_string(),
            vec![
                ("name".to_string(), Ron::Str("a \"q\" \\ \n".to_string())),
                ("value".to_string(), Ron::Float(0.3)),
                ("count".to_string(), Ron::Int(-4)),
                (
                    "points".to_string(),
                    Ron::List(vec![
                        Ron::Tuple(vec![Ron::Float(1.0), Ron::Float(-2.5)]),
                        Ron::Tuple(vec![Ron::Float(1e20), Ron::Float(0.0)]),
                    ]),
                ),
                (
                    "child".to_string(),
                    Ron::Opt(Some(Box::new(Ron::Struct(
                        "Child".to_string(),
                        vec![("kind".to_string(), Ron::Ident("Additive".to_string()))],
                    )))),
                ),
                ("none".to_string(), Ron::Opt(None)),
                ("empty".to_string(), Ron::List(Vec::new())),
                (
                    "items".to_string(),
                    Ron::List(vec![Ron::Struct("Unit".to_string(), Vec::new())]),
                ),
            ],
        )
    }

    #[test]
    fn emit_parse_emit_is_byte_stable() {
        let text = emit_document(&sample());
        let parsed = parse_document(&text).expect("canonical text parses");
        assert_eq!(parsed, sample());
        assert_eq!(emit_document(&parsed), text);
    }

    #[test]
    fn floats_use_the_format_writer() {
        let text = emit_document(&Ron::List(vec![Ron::Float(0.3), Ron::Float(100.0)]));
        assert_eq!(text, "[0.299999999999999989, 100.0]\n");
    }

    #[test]
    fn comments_and_loose_whitespace_parse() {
        let text = "// head\nDoc( /* c */ a : 1 , b:[ 1.5 ,],)\n";
        let parsed = parse_document(text).expect("hand-edited text parses");
        assert_eq!(
            parsed,
            Ron::Struct(
                "Doc".to_string(),
                vec![
                    ("a".to_string(), Ron::Int(1)),
                    ("b".to_string(), Ron::List(vec![Ron::Float(1.5)])),
                ]
            )
        );
    }

    #[test]
    fn hostile_inputs_refuse_typed() {
        let deep = "[".repeat(100) + &"]".repeat(100);
        for text in [
            deep.as_str(),
            "Doc(a: 1, a: 2)",
            "\"unterminated",
            "Doc(a: 1) trailing",
            "Doc(a: 1e999)",
            "Doc(a: 99999999999999999999)",
            "/* open",
            "Some",
            "\"\\u{110000}\"",
            "",
        ] {
            let err = parse_document(text).expect_err(text);
            assert_eq!(err.code(), "TG-AUTH-MALFORMED", "{text}");
        }
    }
}
