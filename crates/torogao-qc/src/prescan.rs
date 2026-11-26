//! Pre-parse scan for non-finite numbers.
//!
//! serde_json (correctly, by its lights) rejects a number
//! literal outside the f64 range — `1e999` — as a parse
//! error, and `NaN` / `Infinity` literals are not JSON at
//! all. But a frozen spec carrying such a value has exactly
//! one contract verdict — `QC_NAN_VALUE` — not a generic
//! parse failure, and the rest of the spec still deserves
//! its checks. So before parsing, this scan walks the raw
//! text with a small structural tokenizer, records every
//! non-finite number token with its JSON Pointer, and
//! rewrites those tokens to `0` in a sanitized copy that
//! serde_json can parse. If the text is too malformed for
//! the tokenizer, the scan abandons itself and serde_json's
//! own verdict on the original text stands.

/// One non-finite token found by the scan.
pub struct NonFinite {
    /// RFC 6901 pointer to the value's node.
    pub pointer: String,
    /// The literal token as written (`1e999`, `NaN`, ...).
    pub token: String,
}

/// The scan's outcome: the text to parse (sanitized when
/// `findings` is non-empty, the original otherwise) and the
/// non-finite tokens found, in document order.
pub struct Prescan {
    /// Text to hand to serde_json.
    pub text: Vec<u8>,
    /// Non-finite tokens found, in document order.
    pub findings: Vec<NonFinite>,
}

/// Scans `bytes` for non-finite number tokens. Never fails:
/// a tokenizer bail-out returns the original text and no
/// findings, leaving malformed input to the real parser.
pub fn prescan(bytes: &[u8]) -> Prescan {
    let mut scanner = Scanner {
        bytes,
        pos: 0,
        hits: Vec::new(),
    };
    let mut segments: Vec<String> = Vec::new();
    if scanner.value(&mut segments).is_err() {
        return Prescan {
            text: bytes.to_vec(),
            findings: Vec::new(),
        };
    }
    scanner.skip_ws();
    if scanner.pos != bytes.len() || scanner.hits.is_empty() {
        // Trailing garbage (malformed — serde_json will say
        // so) or a clean scan: original text, no findings.
        return Prescan {
            text: bytes.to_vec(),
            findings: Vec::new(),
        };
    }
    // Replace hit ranges with `0`, last first so earlier
    // offsets stay valid.
    let mut text = bytes.to_vec();
    let mut ranges: Vec<(usize, usize)> = scanner
        .hits
        .iter()
        .map(|(_, _, start, end)| (*start, *end))
        .collect();
    ranges.sort_unstable_by_key(|(start, _)| std::cmp::Reverse(*start));
    for (start, end) in ranges {
        text.splice(start..end, *b"0");
    }
    Prescan {
        text,
        findings: scanner
            .hits
            .into_iter()
            .map(|(pointer, token, _, _)| NonFinite { pointer, token })
            .collect(),
    }
}

struct Scanner<'a> {
    bytes: &'a [u8],
    pos: usize,
    /// (pointer, token, start, end) per non-finite token.
    hits: Vec<(String, String, usize, usize)>,
}

impl Scanner<'_> {
    fn skip_ws(&mut self) {
        while self.pos < self.bytes.len()
            && matches!(self.bytes[self.pos], b' ' | b'\t' | b'\n' | b'\r')
        {
            self.pos += 1;
        }
    }

    fn peek(&self) -> Result<u8, ()> {
        self.bytes.get(self.pos).copied().ok_or(())
    }

    /// Parses one value at the cursor, tracking the pointer
    /// path in `segments`. Only structure is understood;
    /// strings are skipped, never interpreted beyond escapes.
    fn value(&mut self, segments: &mut Vec<String>) -> Result<(), ()> {
        self.skip_ws();
        match self.peek()? {
            b'{' => {
                self.pos += 1;
                self.skip_ws();
                if self.peek() == Ok(b'}') {
                    self.pos += 1;
                    return Ok(());
                }
                loop {
                    self.skip_ws();
                    let key = self.string()?;
                    self.skip_ws();
                    if self.peek() != Ok(b':') {
                        return Err(());
                    }
                    self.pos += 1;
                    segments.push(key);
                    let result = self.value(segments);
                    segments.pop();
                    result?;
                    self.skip_ws();
                    match self.peek()? {
                        b',' => self.pos += 1,
                        b'}' => {
                            self.pos += 1;
                            return Ok(());
                        }
                        _ => return Err(()),
                    }
                }
            }
            b'[' => {
                self.pos += 1;
                self.skip_ws();
                if self.peek() == Ok(b']') {
                    self.pos += 1;
                    return Ok(());
                }
                let mut index = 0usize;
                loop {
                    segments.push(index.to_string());
                    let result = self.value(segments);
                    segments.pop();
                    result?;
                    index += 1;
                    self.skip_ws();
                    match self.peek()? {
                        b',' => self.pos += 1,
                        b']' => {
                            self.pos += 1;
                            return Ok(());
                        }
                        _ => return Err(()),
                    }
                }
            }
            b'"' => {
                self.string()?;
                Ok(())
            }
            b't' => self.literal("true"),
            b'f' => self.literal("false"),
            b'n' => self.literal("null"),
            _ => self.number(segments),
        }
    }

    /// Skips a string, returning its raw inner text (used for
    /// object keys; escape sequences are kept as written,
    /// which is fine for pointer construction).
    fn string(&mut self) -> Result<String, ()> {
        if self.peek() != Ok(b'"') {
            return Err(());
        }
        self.pos += 1;
        let start = self.pos;
        loop {
            match self.peek()? {
                b'\\' => self.pos += 2,
                b'"' => {
                    let inner = String::from_utf8_lossy(&self.bytes[start..self.pos]).into_owned();
                    self.pos += 1;
                    return Ok(inner);
                }
                _ => self.pos += 1,
            }
        }
    }

    fn literal(&mut self, word: &str) -> Result<(), ()> {
        if self.bytes[self.pos..].starts_with(word.as_bytes()) {
            self.pos += word.len();
            Ok(())
        } else {
            Err(())
        }
    }

    /// Reads one number-ish token (`-1.5e3`, `NaN`,
    /// `-Infinity`, ...) and records it when its f64 value
    /// is not finite.
    fn number(&mut self, segments: &Vec<String>) -> Result<(), ()> {
        let start = self.pos;
        if self.peek() == Ok(b'-') {
            self.pos += 1;
        }
        while self.pos < self.bytes.len()
            && matches!(
                self.bytes[self.pos],
                b'0'..=b'9'
                    | b'.'
                    | b'e'
                    | b'E'
                    | b'+'
                    | b'-'
                    | b'N'
                    | b'a'
                    | b'I'
                    | b'n'
                    | b'f'
                    | b'i'
                    | b't'
                    | b'y'
            )
        {
            self.pos += 1;
        }
        if self.pos == start {
            return Err(());
        }
        let token = String::from_utf8_lossy(&self.bytes[start..self.pos]).into_owned();
        let nonfinite = match token.as_str() {
            "NaN" | "Infinity" | "-Infinity" => true,
            _ => token.parse::<f64>().is_ok_and(|value| !value.is_finite()),
        };
        if nonfinite {
            let mut pointer = String::new();
            for segment in segments {
                pointer.push('/');
                pointer.push_str(&segment.replace('~', "~0").replace('/', "~1"));
            }
            self.hits.push((pointer, token, start, self.pos));
        }
        Ok(())
    }
}
