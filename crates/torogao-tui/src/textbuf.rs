//! A bounded multi-line text buffer: the crate's one editor
//! widget, shared by the verb console, the file editor, and the
//! single-line prompts.
//!
//! Bounds are part of the contract: at most `EDITOR_LINES_MAX`
//! lines of at most `EDITOR_LINE_BYTES_MAX` bytes each. An edit
//! that would cross a bound is refused (returns `false`) and
//! leaves the buffer unchanged — never a panic, never a silent
//! truncation.

/// Maximum lines the editor holds.
pub const EDITOR_LINES_MAX: usize = 4096;
/// Maximum bytes in one editor line.
pub const EDITOR_LINE_BYTES_MAX: usize = 8192;

/// Text with a char-addressed cursor.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextBuf {
    lines: Vec<String>,
    row: usize,
    /// Cursor column, in chars from the line start.
    col: usize,
}

impl TextBuf {
    /// An empty buffer with the cursor at the origin.
    pub fn new() -> Self {
        TextBuf {
            lines: vec![String::new()],
            row: 0,
            col: 0,
        }
    }

    /// A buffer holding `text` (split on `\n`), cursor at origin.
    /// Text past the bounds is refused line by line: the buffer
    /// keeps the first `EDITOR_LINES_MAX` lines, each truncated
    /// at a char boundary within the byte bound — loading a file
    /// is not an edit, so the load stays total and the caller
    /// compares text to notice.
    pub fn from_text(text: &str) -> Self {
        let mut lines: Vec<String> = text
            .split('\n')
            .take(EDITOR_LINES_MAX)
            .map(|line| truncate_bytes(line, EDITOR_LINE_BYTES_MAX).to_string())
            .collect();
        if lines.is_empty() {
            lines.push(String::new());
        }
        TextBuf {
            lines,
            row: 0,
            col: 0,
        }
    }

    /// The buffer's text, lines joined with `\n`.
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    /// All lines, for rendering.
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// The cursor, as `(row, col_chars)`.
    pub fn cursor(&self) -> (usize, usize) {
        (self.row, self.col)
    }

    /// Removes every line; cursor returns to the origin.
    pub fn clear(&mut self) {
        self.lines = vec![String::new()];
        self.row = 0;
        self.col = 0;
    }

    /// Inserts one char at the cursor. Refused (returns `false`,
    /// buffer unchanged) when the line is at the byte bound.
    pub fn insert_char(&mut self, ch: char) -> bool {
        let line = &self.lines[self.row];
        if line.len() + ch.len_utf8() > EDITOR_LINE_BYTES_MAX {
            return false;
        }
        let at = byte_at(line, self.col);
        self.lines[self.row].insert(at, ch);
        self.col += 1;
        true
    }

    /// Splits the current line at the cursor. Refused when the
    /// buffer already holds `EDITOR_LINES_MAX` lines.
    pub fn insert_newline(&mut self) -> bool {
        if self.lines.len() >= EDITOR_LINES_MAX {
            return false;
        }
        let line = self.lines[self.row].clone();
        let at = byte_at(&line, self.col);
        let (head, tail) = line.split_at(at);
        self.lines[self.row] = head.to_string();
        self.lines.insert(self.row + 1, tail.to_string());
        self.row += 1;
        self.col = 0;
        true
    }

    /// Deletes the char before the cursor, joining lines at a
    /// line start. A no-op at the buffer origin.
    pub fn backspace(&mut self) {
        if self.col > 0 {
            let line = self.lines[self.row].clone();
            let end = byte_at(&line, self.col);
            let start = byte_at(&line, self.col - 1);
            self.lines[self.row].replace_range(start..end, "");
            self.col -= 1;
        } else if self.row > 0 {
            let tail = self.lines.remove(self.row);
            self.row -= 1;
            self.col = self.lines[self.row].chars().count();
            self.lines[self.row].push_str(&tail);
        }
    }

    /// Deletes the char at the cursor, joining the next line at
    /// a line end. A no-op at the buffer end.
    pub fn delete(&mut self) {
        let line = self.lines[self.row].clone();
        if self.col < line.chars().count() {
            let start = byte_at(&line, self.col);
            let end = byte_at(&line, self.col + 1);
            self.lines[self.row].replace_range(start..end, "");
        } else if self.row + 1 < self.lines.len() {
            let tail = self.lines.remove(self.row + 1);
            self.lines[self.row].push_str(&tail);
        }
    }

    /// Cursor up one line, column clamped to the line's length.
    pub fn move_up(&mut self) {
        if self.row > 0 {
            self.row -= 1;
            self.clamp_col();
        }
    }

    /// Cursor down one line, column clamped.
    pub fn move_down(&mut self) {
        if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.clamp_col();
        }
    }

    /// Cursor left one char (no line wrap).
    pub fn move_left(&mut self) {
        self.col = self.col.saturating_sub(1);
    }

    /// Cursor right one char (no line wrap).
    pub fn move_right(&mut self) {
        if self.col < self.lines[self.row].chars().count() {
            self.col += 1;
        }
    }

    /// Cursor to the line start.
    pub fn move_home(&mut self) {
        self.col = 0;
    }

    /// Cursor to the line end.
    pub fn move_end(&mut self) {
        self.col = self.lines[self.row].chars().count();
    }

    /// Cursor to the buffer start.
    pub fn move_top(&mut self) {
        self.row = 0;
        self.col = 0;
    }

    fn clamp_col(&mut self) {
        let len = self.lines[self.row].chars().count();
        if self.col > len {
            self.col = len;
        }
    }
}

/// The byte index of char column `col` in `line` (the line's byte
/// length when `col` is past the end).
fn byte_at(line: &str, col: usize) -> usize {
    line.char_indices()
        .nth(col)
        .map_or(line.len(), |(index, _)| index)
}

/// The longest prefix of `text` within `bytes_max` bytes, cut at
/// a char boundary.
fn truncate_bytes(text: &str, bytes_max: usize) -> &str {
    if text.len() <= bytes_max {
        return text;
    }
    let mut end = bytes_max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[cfg(test)]
mod tests {
    use super::{EDITOR_LINE_BYTES_MAX, TextBuf};

    #[test]
    fn typing_and_joining_round_trip() {
        let mut buf = TextBuf::new();
        for ch in "ab".chars() {
            assert!(buf.insert_char(ch));
        }
        assert!(buf.insert_newline());
        assert!(buf.insert_char('c'));
        assert_eq!(buf.text(), "ab\nc");
        buf.move_up();
        buf.move_end();
        buf.backspace();
        assert_eq!(buf.text(), "a\nc");
        buf.move_down();
        buf.move_home();
        buf.backspace();
        assert_eq!(buf.text(), "ac");
    }

    #[test]
    fn line_byte_bound_refuses_without_loss() {
        let mut buf = TextBuf::new();
        let wide = 'é';
        let mut accepted = 0;
        while buf.insert_char(wide) {
            accepted += 1;
        }
        assert_eq!(accepted, EDITOR_LINE_BYTES_MAX / wide.len_utf8());
        assert_eq!(buf.text().len(), EDITOR_LINE_BYTES_MAX);
    }

    #[test]
    fn from_text_keeps_short_text_exact() {
        let buf = TextBuf::from_text("one\ntwo\n");
        assert_eq!(buf.text(), "one\ntwo\n");
    }
}
