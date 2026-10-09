use std::collections::VecDeque;
use vte::{Params, Parser, Perform};

pub trait Screen {
    fn feed(&mut self, bytes: &[u8]);
    fn resize(&mut self, cols: u16, rows: u16);
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
    wrap_next: bool,
    saved: (usize, usize),
    top: usize,
    bottom: usize,
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
        if let Some((screen, row, col)) = &mut self.main {
            for line in screen.iter_mut() {
                line.resize(cols, ' ');
            }
            screen.resize(rows, vec![' '; cols]);
            (*row, *col) = ((*row).min(rows - 1), (*col).min(cols - 1));
        }
        self.cols = cols;
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

    fn keep(&mut self, line: Vec<char>) {
        if self.main.is_some() {
            return;
        }
        self.scrollback.push_back(line.iter().collect::<String>().trim_end().to_string());
        if self.scrollback.len() > SCROLLBACK {
            self.scrollback.pop_front();
        }
    }

    fn region(&mut self) -> usize {
        self.bottom = self.bottom.min(self.rows - 1);
        if self.top > self.bottom {
            (self.top, self.bottom) = (0, self.rows - 1);
        }
        self.bottom - self.top + 1
    }

    fn scroll_up(&mut self, n: usize) {
        for _ in 0..n.min(self.region()) {
            let line = self.lines.remove(self.top);
            if self.top == 0 {
                self.keep(line);
            }
            self.lines.insert(self.bottom, self.blank());
        }
    }

    fn scroll_down(&mut self, n: usize) {
        for _ in 0..n.min(self.region()) {
            self.lines.remove(self.bottom);
            self.lines.insert(self.top, self.blank());
        }
    }

    fn linefeed(&mut self) {
        self.region();
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
        let Some(line) = self.lines.get_mut(row) else { return };
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
                let cols = self.cols;
                self.lines.iter_mut().for_each(|line| line.resize(cols, ' '));
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
        let (row, col) = (self.row.min(self.rows - 1), self.col.min(self.cols - 1));
        if let Some(cell) = self.lines.get_mut(row).and_then(|line| line.get_mut(col)) {
            *cell = c;
        }
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
            'B' | 'e' => self.move_to(row.saturating_add(n), col),
            'C' | 'a' => self.move_to(row, col.saturating_add(n)),
            'D' => self.move_to(row, col.saturating_sub(n)),
            'E' => self.move_to(row.saturating_add(n), 0),
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
                    self.erase(row, 0, col.saturating_add(1));
                }
                2 => (0..self.rows).for_each(|r| self.erase(r, 0, self.cols)),
                3 => self.scrollback.clear(),
                _ => {}
            },
            'K' => match values.first().copied().unwrap_or(0) {
                0 => self.erase(row, col, self.cols),
                1 => self.erase(row, 0, col.saturating_add(1)),
                2 => self.erase(row, 0, self.cols),
                _ => {}
            },
            'L' if self.region() > 0 && (self.top..=self.bottom).contains(&row) => {
                for _ in 0..n.min(self.bottom - row + 1) {
                    self.lines.remove(self.bottom);
                    self.lines.insert(row, self.blank());
                }
            }
            'M' if self.region() > 0 && (self.top..=self.bottom).contains(&row) => {
                for _ in 0..n.min(self.bottom - row + 1) {
                    self.lines.remove(row);
                    self.lines.insert(self.bottom, self.blank());
                }
            }
            'P' => {
                let cols = self.cols;
                if let Some(line) = self.lines.get_mut(row).filter(|l| col < l.len()) {
                    let n = n.min(line.len() - col);
                    line.drain(col..col + n);
                    line.resize(cols, ' ');
                }
            }
            '@' => {
                let cols = self.cols;
                if let Some(line) = self.lines.get_mut(row).filter(|l| col <= l.len()) {
                    let n = n.min(cols.saturating_sub(col));
                    line.splice(col..col, std::iter::repeat_n(' ', n));
                    line.truncate(cols);
                }
            }
            'X' => self.erase(row, col, col.saturating_add(n)),
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
    fn resizing_under_the_alternate_screen_resizes_the_main_one() {
        let mut s = screen(10, 3, "main\r\n\x1b[?1049h");
        s.resize(4, 2);
        s.feed(b"\x1b[?1049l\x1b[9;9Hxy");
        s.resize(20, 6);
        s.feed(b"\x1b[?1049h\x1b[?1049l\x1b[20;20Hz");
        assert_eq!(s.text(Some(100)), "main\n   x\ny\n\n\n\n                   z");
    }

    #[test]
    fn hostile_output_never_panics() {
        let nasty: [&[u8]; 16] = [
            b"\x1b[99999999;99999999H", b"\x1b[0;0r", b"\x1b[65535;1r", b"\x1b[5;3r", b"\x1b[99999L", b"\x1b[99999M",
            b"\x1b[99999P", b"\x1b[99999@", b"\x1b[99999X", b"\x1b[99999S", b"\x1b[99999T", b"\x1b[99999A\x1b[99999D",
            b"\x1b[?1049h\x1b[?1049h\x1b[?47l\x1b[?1049l", b"\x1bM\x1bM\x1bD\x1bE\x1b8\x1bc", b"\xff\xfe\xc3\x28\xf0\x28\x8c\x28",
            b"\x1b[;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;H",
        ];
        let mut s = TextScreen::new(80, 24);
        for bytes in nasty {
            s.feed(bytes);
            s.feed("wide 字 and e\u{301} ".as_bytes());
        }
        let mut seed: u64 = 0x9e3779b97f4a7c15;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let alphabet = b"\x1b[]?;0123456789HJKLMPSTX@ABCDEFGdfhlrsu78Dc\r\n\x08\t\x07ab";
        for round in 0..3000 {
            let len = (next() % 300) as usize;
            let bytes: Vec<u8> = (0..len)
                .map(|_| {
                    let r = next();
                    if r % 4 == 0 { (r >> 8) as u8 } else { alphabet[(r >> 8) as usize % alphabet.len()] }
                })
                .collect();
            s.feed(&bytes);
            if round % 50 == 0 {
                s.resize((next() % 300) as u16, (next() % 100) as u16);
            }
            let _ = s.text(None);
            let _ = s.text(Some((next() % 50) as usize + 1));
        }
    }

    #[test]
    fn resizing_keeps_the_cursor_line() {
        let mut s = screen(10, 4, "a\r\nb\r\nc\r\nd");
        s.resize(5, 2);
        assert_eq!(s.text(None), "c\nd");
        assert_eq!(s.text(Some(10)), "a\nb\nc\nd");
    }
}
