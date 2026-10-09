//! The daemon's copy of each terminal's screen, for `read`. For M0 a small
//! text grid behind the `Screen` trait: characters only (no colours or
//! attributes), the usual cursor, erase and scroll sequences, the alternate
//! screen, and plain-text scrollback. libghostty-vt replaces it in M1.

use std::collections::VecDeque;
use vte::{Params, Parser, Perform};

pub trait Screen {
    /// Output from the program, as it arrives.
    fn feed(&mut self, bytes: &[u8]);
    fn resize(&mut self, cols: u16, rows: u16);
    /// The screen, one line per row, without trailing blanks or blank rows at
    /// the bottom; with `lines`, the last `lines` lines, counting scrollback.
    fn text(&self, lines: Option<usize>) -> String;
}

const SCROLLBACK: usize = 10_000;

pub struct TextScreen {
    parser: Parser,
    grid: Grid,
}

impl TextScreen {
    pub fn new(cols: u16, rows: u16) -> TextScreen {
        TextScreen { parser: Parser::new(), grid: Grid::new(cols.max(1) as usize, rows.max(1) as usize) }
    }
}

impl Screen for TextScreen {
    fn feed(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.grid, bytes);
    }

    fn resize(&mut self, cols: u16, rows: u16) {
        self.grid.resize(cols.max(1) as usize, rows.max(1) as usize);
    }

    fn text(&self, lines: Option<usize>) -> String {
        let row = |cells: &Vec<char>| cells.iter().collect::<String>().trim_end().to_string();
        let mut out: Vec<String> = match lines {
            None => self.grid.lines.iter().map(row).collect(),
            Some(_) => self.grid.scrollback.iter().cloned().chain(self.grid.lines.iter().map(row)).collect(),
        };
        while out.last().is_some_and(String::is_empty) {
            out.pop();
        }
        if let Some(n) = lines {
            out.drain(..out.len().saturating_sub(n));
        }
        out.join("\n")
    }
}

struct Grid {
    cols: usize,
    rows: usize,
    lines: Vec<Vec<char>>,
    scrollback: VecDeque<String>,
    row: usize,
    col: usize,
    /// The last column was written: the next character wraps first.
    wrap_next: bool,
    saved: (usize, usize),
    /// The scroll region, inclusive.
    top: usize,
    bottom: usize,
    /// The main screen and cursor while the alternate one shows.
    main: Option<(Vec<Vec<char>>, usize, usize)>,
}

impl Grid {
    fn new(cols: usize, rows: usize) -> Grid {
        Grid {
            cols,
            rows,
            lines: vec![vec![' '; cols]; rows],
            scrollback: VecDeque::new(),
            row: 0,
            col: 0,
            wrap_next: false,
            saved: (0, 0),
            top: 0,
            bottom: rows - 1,
            main: None,
        }
    }

    fn blank(&self) -> Vec<char> {
        vec![' '; self.cols]
    }

    fn resize(&mut self, cols: usize, rows: usize) {
        for line in &mut self.lines {
            line.resize(cols, ' ');
        }
        self.cols = cols;
        // Shrinking keeps the cursor's row: lines above it go to scrollback.
        while self.lines.len() > rows && self.row > 0 {
            let line = self.lines.remove(0);
            self.keep(line);
            self.row -= 1;
        }
        self.lines.truncate(rows);
        while self.lines.len() < rows {
            self.lines.push(self.blank());
        }
        self.rows = rows;
        (self.top, self.bottom) = (0, rows - 1);
        self.row = self.row.min(rows - 1);
        self.col = self.col.min(cols - 1);
        self.wrap_next = false;
    }

    /// A line scrolled off the top of the main screen.
    fn keep(&mut self, line: Vec<char>) {
        if self.main.is_some() {
            return;
        }
        self.scrollback.push_back(line.iter().collect::<String>().trim_end().to_string());
        if self.scrollback.len() > SCROLLBACK {
            self.scrollback.pop_front();
        }
    }

    fn scroll_up(&mut self, n: usize) {
        for _ in 0..n.min(self.bottom - self.top + 1) {
            let line = self.lines.remove(self.top);
            if self.top == 0 {
                self.keep(line);
            }
            self.lines.insert(self.bottom, self.blank());
        }
    }

    fn scroll_down(&mut self, n: usize) {
        for _ in 0..n.min(self.bottom - self.top + 1) {
            self.lines.remove(self.bottom);
            self.lines.insert(self.top, self.blank());
        }
    }

    fn linefeed(&mut self) {
        if self.row == self.bottom {
            self.scroll_up(1);
        } else if self.row + 1 < self.rows {
            self.row += 1;
        }
    }

    fn move_to(&mut self, row: usize, col: usize) {
        self.row = row.min(self.rows - 1);
        self.col = col.min(self.cols - 1);
        self.wrap_next = false;
    }

    fn erase(&mut self, row: usize, from: usize, to: usize) {
        let line = &mut self.lines[row];
        for cell in line.iter_mut().take(to.min(self.cols)).skip(from) {
            *cell = ' ';
        }
    }

    fn alternate(&mut self, on: bool) {
        if on && self.main.is_none() {
            let screen = std::mem::replace(&mut self.lines, vec![vec![' '; self.cols]; self.rows]);
            self.main = Some((screen, self.row, self.col));
        } else if !on {
            if let Some((screen, row, col)) = self.main.take() {
                self.lines = screen;
                self.lines.resize(self.rows, self.blank());
                self.move_to(row, col);
            }
        }
    }
}

impl Perform for Grid {
    fn print(&mut self, c: char) {
        if self.wrap_next {
            self.col = 0;
            self.wrap_next = false;
            self.linefeed();
        }
        self.lines[self.row][self.col] = c;
        if self.col + 1 >= self.cols {
            self.wrap_next = true;
        } else {
            self.col += 1;
        }
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\n' | 0x0B | 0x0C => self.linefeed(),
            b'\r' => self.move_to(self.row, 0),
            0x08 => self.move_to(self.row, self.col.saturating_sub(1)),
            b'\t' => self.move_to(self.row, (self.col / 8 + 1) * 8),
            _ => {}
        }
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], _ignore: bool, action: char) {
        let values: Vec<usize> = params.iter().map(|p| p.first().copied().unwrap_or(0) as usize).collect();
        let arg = |i: usize, default: usize| values.get(i).copied().filter(|v| *v != 0).unwrap_or(default);
        let n = arg(0, 1);
        let private = intermediates.first() == Some(&b'?');
        let (row, col) = (self.row, self.col);
        match action {
            'A' => self.move_to(row.saturating_sub(n), col),
            'B' | 'e' => self.move_to(row + n, col),
            'C' | 'a' => self.move_to(row, col + n),
            'D' => self.move_to(row, col.saturating_sub(n)),
            'E' => self.move_to(row + n, 0),
            'F' => self.move_to(row.saturating_sub(n), 0),
            'G' | '`' => self.move_to(row, n - 1),
            'd' => self.move_to(n - 1, col),
            'H' | 'f' => self.move_to(arg(0, 1) - 1, arg(1, 1) - 1),
            'J' => match values.first().copied().unwrap_or(0) {
                0 => {
                    self.erase(row, col, self.cols);
                    (row + 1..self.rows).for_each(|r| self.erase(r, 0, self.cols));
                }
                1 => {
                    (0..row).for_each(|r| self.erase(r, 0, self.cols));
                    self.erase(row, 0, col + 1);
                }
                2 => (0..self.rows).for_each(|r| self.erase(r, 0, self.cols)),
                3 => self.scrollback.clear(),
                _ => {}
            },
            'K' => match values.first().copied().unwrap_or(0) {
                0 => self.erase(row, col, self.cols),
                1 => self.erase(row, 0, col + 1),
                2 => self.erase(row, 0, self.cols),
                _ => {}
            },
            'L' if (self.top..=self.bottom).contains(&row) => {
                for _ in 0..n.min(self.bottom - row + 1) {
                    self.lines.remove(self.bottom);
                    self.lines.insert(row, self.blank());
                }
            }
            'M' if (self.top..=self.bottom).contains(&row) => {
                for _ in 0..n.min(self.bottom - row + 1) {
                    self.lines.remove(row);
                    self.lines.insert(self.bottom, self.blank());
                }
            }
            'P' => {
                let line = &mut self.lines[row];
                let n = n.min(line.len() - col);
                line.drain(col..col + n);
                line.resize(self.cols, ' ');
            }
            '@' => {
                let line = &mut self.lines[row];
                for _ in 0..n.min(self.cols - col) {
                    line.insert(col, ' ');
                }
                line.truncate(self.cols);
            }
            'X' => self.erase(row, col, col + n),
            'S' if !private => self.scroll_up(n),
            'T' if !private => self.scroll_down(n),
            'r' if !private => {
                let (top, bottom) = (arg(0, 1) - 1, arg(1, self.rows).min(self.rows) - 1);
                if top < bottom {
                    (self.top, self.bottom) = (top, bottom);
                    self.move_to(0, 0);
                }
            }
            's' if !private => self.saved = (row, col),
            'u' if !private => self.move_to(self.saved.0, self.saved.1),
            'h' | 'l' if private => {
                for mode in &values {
                    if matches!(mode, 47 | 1047 | 1049) {
                        if *mode == 1049 && action == 'h' {
                            self.saved = (self.row, self.col);
                        }
                        self.alternate(action == 'h');
                        if *mode == 1049 && action == 'l' {
                            self.move_to(self.saved.0, self.saved.1);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], _ignore: bool, byte: u8) {
        if !intermediates.is_empty() {
            return;
        }
        match byte {
            b'7' => self.saved = (self.row, self.col),
            b'8' => self.move_to(self.saved.0, self.saved.1),
            b'D' => self.linefeed(),
            b'E' => {
                self.linefeed();
                self.move_to(self.row, 0);
            }
            b'M' => {
                if self.row == self.top {
                    self.scroll_down(1);
                } else {
                    self.move_to(self.row.saturating_sub(1), self.col);
                }
            }
            b'c' => *self = Grid::new(self.cols, self.rows),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(cols: u16, rows: u16, output: &str) -> TextScreen {
        let mut screen = TextScreen::new(cols, rows);
        screen.feed(output.as_bytes());
        screen
    }

    #[test]
    fn lines_and_prompts() {
        let s = screen(20, 5, "$ echo hi\r\nhi\r\n$ ");
        assert_eq!(s.text(None), "$ echo hi\nhi\n$");
    }

    #[test]
    fn colours_and_titles_are_not_text() {
        let s = screen(20, 3, "\x1b]0;title\x07\x1b[1;32mgreen\x1b[0m plain");
        assert_eq!(s.text(None), "green plain");
    }

    #[test]
    fn cursor_moves_and_erases() {
        let s = screen(20, 12, "junk\x1b[2J\x1b[3;5HS4 TOP\x1b[10;20HS4 MIDDLE");
        let text = s.text(None);
        let lines: Vec<&str> = text.split('\n').collect();
        assert_eq!(lines[2], "    S4 TOP");
        // Row 10 has room for one character at column 20: the rest wraps.
        assert_eq!(lines[9], format!("{}S", " ".repeat(19)));
        assert_eq!(lines[10], "4 MIDDLE");
        let s = screen(10, 2, "abcdef\x1b[3D\x1b[K");
        assert_eq!(s.text(None), "abc");
        let s = screen(10, 2, "abcdef\r\x1b[2P");
        assert_eq!(s.text(None), "cdef");
    }

    #[test]
    fn scrolling_keeps_scrollback() {
        let s = screen(10, 3, "1\r\n2\r\n3\r\n4\r\n5");
        assert_eq!(s.text(None), "3\n4\n5");
        assert_eq!(s.text(Some(4)), "2\n3\n4\n5");
        assert_eq!(s.text(Some(100)), "1\n2\n3\n4\n5");
    }

    #[test]
    fn the_alternate_screen_comes_and_goes() {
        let mut s = screen(10, 3, "shell\r\n");
        s.feed(b"\x1b[?1049h\x1b[Hfull screen");
        assert_eq!(s.text(None), "full scree\nn");
        s.feed(b"\x1b[?1049l");
        assert_eq!(s.text(None), "shell");
    }

    #[test]
    fn resizing_keeps_the_cursor_line() {
        let mut s = screen(10, 4, "a\r\nb\r\nc\r\nd");
        s.resize(5, 2);
        assert_eq!(s.text(None), "c\nd");
        assert_eq!(s.text(Some(10)), "a\nb\nc\nd");
    }
}
