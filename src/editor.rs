use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Multi-line text with a cursor; `col` counts chars, not bytes.
#[derive(Debug, Default)]
pub struct Editor {
    pub lines: Vec<String>,
    pub row: usize,
    pub col: usize,
}

impl Editor {
    pub fn new() -> Self {
        Editor { lines: vec![String::new()], row: 0, col: 0 }
    }

    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    pub fn handle(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => self.insert(c),
            KeyCode::Tab => (0..4).for_each(|_| self.insert(' ')),
            KeyCode::Enter => self.newline(),
            KeyCode::Backspace => self.backspace(),
            KeyCode::Delete => self.delete(),
            KeyCode::Left => self.left(),
            KeyCode::Right => self.right(),
            KeyCode::Up => self.vertical(-1),
            KeyCode::Down => self.vertical(1),
            KeyCode::Home => self.col = 0,
            KeyCode::End => self.col = self.line_len(),
            _ => {}
        }
    }

    pub fn insert(&mut self, c: char) {
        let at = self.byte_at(self.col);
        self.lines[self.row].insert(at, c);
        self.col += 1;
    }

    pub fn newline(&mut self) {
        let at = self.byte_at(self.col);
        let rest = self.lines[self.row].split_off(at);
        self.row += 1;
        self.lines.insert(self.row, rest);
        self.col = 0;
    }

    pub fn backspace(&mut self) {
        if self.col > 0 {
            self.col -= 1;
            let at = self.byte_at(self.col);
            self.lines[self.row].remove(at);
        } else if self.row > 0 {
            let line = self.lines.remove(self.row);
            self.row -= 1;
            self.col = self.line_len();
            self.lines[self.row].push_str(&line);
        }
    }

    pub fn delete(&mut self) {
        if self.col < self.line_len() {
            let at = self.byte_at(self.col);
            self.lines[self.row].remove(at);
        } else if self.row + 1 < self.lines.len() {
            let next = self.lines.remove(self.row + 1);
            self.lines[self.row].push_str(&next);
        }
    }

    fn left(&mut self) {
        if self.col > 0 {
            self.col -= 1;
        } else if self.row > 0 {
            self.row -= 1;
            self.col = self.line_len();
        }
    }

    fn right(&mut self) {
        if self.col < self.line_len() {
            self.col += 1;
        } else if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.col = 0;
        }
    }

    fn vertical(&mut self, delta: isize) {
        let target = self.row as isize + delta;
        if target < 0 || target as usize >= self.lines.len() {
            return;
        }
        self.row = target as usize;
        self.col = self.col.min(self.line_len());
    }

    fn line_len(&self) -> usize {
        self.lines[self.row].chars().count()
    }

    fn byte_at(&self, col: usize) -> usize {
        let line = &self.lines[self.row];
        line.char_indices().nth(col).map_or(line.len(), |(i, _)| i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(text: &str) -> Editor {
        let mut editor = Editor::new();
        for c in text.chars() {
            if c == '\n' { editor.newline() } else { editor.insert(c) }
        }
        editor
    }

    #[test]
    fn inserts_and_deletes_across_lines() {
        let mut editor = typed("sełect\n1");
        assert_eq!((editor.row, editor.col), (1, 1));
        editor.col = 0;
        editor.backspace();
        assert_eq!(editor.text(), "sełect1");
        assert_eq!((editor.row, editor.col), (0, 6));
        editor.newline();
        editor.vertical(-1);
        editor.col = 3;
        editor.backspace();
        assert_eq!(editor.text(), "seect\n1");
        editor.col = editor.line_len();
        editor.delete();
        assert_eq!(editor.lines, ["seect1"]);
    }
}
