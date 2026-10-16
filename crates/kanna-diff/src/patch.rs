use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Modified,
    Added,
    Deleted,
    Renamed,
}

impl Status {
    pub fn letter(self) -> &'static str {
        match self {
            Status::Modified => "M",
            Status::Added => "A",
            Status::Deleted => "D",
            Status::Renamed => "R",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Context,
    Added,
    Removed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub kind: Kind,
    pub text: String,
    pub old: Option<u32>,
    pub new: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hunk {
    pub old_start: u32,
    pub new_start: u32,
    pub scope: String,
    pub lines: Vec<Line>,
}

impl Hunk {
    pub fn old_end(&self) -> u32 {
        self.old_start + self.lines.iter().filter(|l| l.kind != Kind::Added).count() as u32
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fold {
    LockFile,
    Generated,
    Large(u32),
}

impl Fold {
    pub fn message(&self, path: &str) -> String {
        let name = path.rsplit('/').next().unwrap_or(path);
        match self {
            Fold::LockFile => format!("{name} is a lock file: not shown."),
            Fold::Generated => format!("{name} is generated: not shown."),
            Fold::Large(lines) => format!("{} changed lines: not shown.", thousands(*lines)),
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Fold::LockFile => "lock",
            Fold::Generated => "generated",
            Fold::Large(_) => "large",
        }
    }
}

fn thousands(n: u32) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct File {
    pub path: String,
    pub old_path: Option<String>,
    pub status: Status,
    pub added: u32,
    pub removed: u32,
    pub binary: bool,
    pub untracked: bool,
    pub fold: Option<Fold>,
    pub hunks: Vec<Hunk>,
}

impl File {
    pub fn new(path: &str, status: Status) -> File {
        File { path: path.into(), old_path: None, status, added: 0, removed: 0, binary: false, untracked: false, fold: None, hunks: vec![] }
    }

    pub fn to_json(&self) -> Value {
        let hunks: Vec<Value> = self
            .hunks
            .iter()
            .map(|h| {
                let lines: Vec<Value> = h
                    .lines
                    .iter()
                    .map(|l| {
                        let kind = match l.kind {
                            Kind::Context => " ",
                            Kind::Added => "+",
                            Kind::Removed => "-",
                        };
                        json!([kind, l.text, l.old, l.new])
                    })
                    .collect();
                json!({ "old": h.old_start, "new": h.new_start, "scope": h.scope, "lines": lines })
            })
            .collect();
        let mut out = json!({
            "path": self.path, "status": self.status.letter(), "added": self.added, "removed": self.removed, "hunks": hunks,
        });
        if let Some(old) = &self.old_path {
            out["oldPath"] = json!(old);
        }
        if self.binary {
            out["binary"] = json!(true);
        }
        if self.untracked {
            out["untracked"] = json!(true);
        }
        if let Some(fold) = &self.fold {
            out["fold"] = json!(fold.name());
            out["note"] = json!(fold.message(&self.path));
        }
        out
    }
}

pub fn parse(text: &str) -> Vec<File> {
    let mut files: Vec<File> = vec![];
    let mut file: Option<File> = None;
    let mut hunk: Option<Hunk> = None;
    let (mut old, mut new) = (0u32, 0u32);

    fn close_hunk(file: &mut Option<File>, hunk: &mut Option<Hunk>) {
        if let (Some(f), Some(h)) = (file.as_mut(), hunk.take()) {
            f.hunks.push(h);
        }
    }
    fn close_file(files: &mut Vec<File>, file: &mut Option<File>, hunk: &mut Option<Hunk>) {
        close_hunk(file, hunk);
        if let Some(mut done) = file.take() {
            for line in done.hunks.iter().flat_map(|h| &h.lines) {
                match line.kind {
                    Kind::Added => done.added += 1,
                    Kind::Removed => done.removed += 1,
                    Kind::Context => {}
                }
            }
            files.push(done);
        }
    }

    for line in text.split('\n') {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            close_file(&mut files, &mut file, &mut hunk);
            let (a, b) = header_paths(rest);
            let mut f = File::new(b.as_deref().or(a.as_deref()).unwrap_or(""), Status::Modified);
            if let (Some(a), Some(b)) = (&a, &b) {
                if a != b {
                    f.old_path = Some(a.clone());
                }
            }
            file = Some(f);
            continue;
        }
        let Some(f) = file.as_mut() else { continue };
        if let Some(h) = hunk.as_mut() {
            let mut chars = line.chars();
            let first = chars.next();
            if matches!(first, Some(' ' | '+' | '-')) {
                let body = chars.as_str().to_string();
                match first {
                    Some('+') => {
                        h.lines.push(Line { kind: Kind::Added, text: body, old: None, new: Some(new) });
                        new += 1;
                    }
                    Some('-') => {
                        h.lines.push(Line { kind: Kind::Removed, text: body, old: Some(old), new: None });
                        old += 1;
                    }
                    _ => {
                        h.lines.push(Line { kind: Kind::Context, text: body, old: Some(old), new: Some(new) });
                        old += 1;
                        new += 1;
                    }
                }
                continue;
            }
        }
        if line.starts_with('\\') {
            continue;
        }
        if line.starts_with("@@ ") {
            close_hunk(&mut file, &mut hunk);
            if let Some((o, n, scope)) = hunk_header(line) {
                hunk = Some(Hunk { old_start: o, new_start: n, scope, lines: vec![] });
                old = o;
                new = n;
            }
            continue;
        }
        if line.starts_with("new file mode") {
            f.status = Status::Added;
        } else if line.starts_with("deleted file mode") {
            f.status = Status::Deleted;
        } else if let Some(from) = line.strip_prefix("rename from ") {
            f.old_path = Some(unquote(from));
            f.status = Status::Renamed;
        } else if let Some(to) = line.strip_prefix("rename to ") {
            f.path = unquote(to);
            f.status = Status::Renamed;
        } else if let Some(rest) = line.strip_prefix("+++ ") {
            if let Some(path) = side(rest, "b/") {
                f.path = path;
            }
        } else if let Some(rest) = line.strip_prefix("--- ") {
            if let Some(path) = side(rest, "a/") {
                if f.old_path.is_none() && path != f.path {
                    f.old_path = Some(path);
                }
            }
        } else if line.starts_with("Binary files ") || line == "GIT binary patch" {
            f.binary = true;
        }
    }
    close_file(&mut files, &mut file, &mut hunk);
    files
}

fn side(rest: &str, prefix: &str) -> Option<String> {
    if rest == "/dev/null" {
        return None;
    }
    let path = unquote(rest);
    Some(path.strip_prefix(prefix).map(String::from).unwrap_or(path))
}

fn header_paths(rest: &str) -> (Option<String>, Option<String>) {
    let strip = |p: String, prefix: &str| p.strip_prefix(prefix).map(String::from).unwrap_or(p);
    if rest.starts_with('"') {
        let parts = quoted_parts(rest);
        return (parts.first().cloned().map(|p| strip(p, "a/")), parts.get(1).cloned().map(|p| strip(p, "b/")));
    }
    let chars: Vec<char> = rest.chars().collect();
    if chars.len() % 2 == 1 {
        let half = (chars.len() - 1) / 2;
        let a: String = chars[..half].iter().collect();
        let b: String = chars[half + 1..].iter().collect();
        if let (Some(a), Some(b)) = (a.strip_prefix("a/"), b.strip_prefix("b/")) {
            if a == b {
                return (Some(a.into()), Some(b.into()));
            }
        }
    }
    match rest.find(" b/") {
        Some(i) => (Some(strip(rest[..i].into(), "a/")), Some(rest[i + 3..].into())),
        None => (Some(strip(rest.into(), "a/")), None),
    }
}

fn quoted_parts(text: &str) -> Vec<String> {
    let mut parts = vec![];
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        while i < bytes.len() && bytes[i] == b' ' {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let start = i;
        if bytes[i] == b'"' {
            i += 1;
            let mut escaped = false;
            while i < bytes.len() {
                if escaped {
                    escaped = false;
                } else if bytes[i] == b'\\' {
                    escaped = true;
                } else if bytes[i] == b'"' {
                    break;
                }
                i += 1;
            }
            i = (i + 1).min(bytes.len());
            parts.push(unquote(&text[start..i]));
        } else {
            while i < bytes.len() && bytes[i] != b' ' {
                i += 1;
            }
            parts.push(text[start..i].into());
        }
    }
    parts
}

pub fn unquote(text: &str) -> String {
    if text.len() < 2 || !text.starts_with('"') || !text.ends_with('"') {
        return text.into();
    }
    let inner = &text.as_bytes()[1..text.len() - 1];
    let mut out = vec![];
    let mut i = 0;
    while i < inner.len() {
        if inner[i] == b'\\' && i + 1 < inner.len() {
            let n = inner[i + 1];
            match n {
                b'n' => {
                    out.push(b'\n');
                    i += 2;
                }
                b't' => {
                    out.push(b'\t');
                    i += 2;
                }
                b'0'..=b'7' if i + 4 <= inner.len() => {
                    let d = |k: usize| inner[i + k].wrapping_sub(b'0');
                    out.push(d(1) << 6 | d(2) << 3 | d(3));
                    i += 4;
                }
                _ => {
                    out.push(n);
                    i += 2;
                }
            }
        } else {
            out.push(inner[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hunk_header(line: &str) -> Option<(u32, u32, String)> {
    let mut parts = line.splitn(3, "@@");
    parts.next()?;
    let ranges = parts.next()?;
    let scope = parts.next().unwrap_or("").trim().to_string();
    let start = |sign: char| ranges.split(' ').find(|r| r.starts_with(sign)).and_then(|r| r[1..].split(',').next()?.parse::<u32>().ok());
    Some((start('-')?.max(1), start('+')?.max(1), scope))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Numstat {
    pub path: String,
    pub old_path: Option<String>,
    pub added: u32,
    pub removed: u32,
    pub binary: bool,
}

pub fn numstat(text: &str) -> Vec<Numstat> {
    let mut out = vec![];
    let mut fields = text.split('\0');
    while let Some(head) = fields.next() {
        let line = head.trim_start_matches('\n');
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        if parts.len() != 3 {
            continue;
        }
        let binary = parts[0] == "-";
        let (added, removed) = (parts[0].parse().unwrap_or(0), parts[1].parse().unwrap_or(0));
        if parts[2].is_empty() {
            let (Some(from), Some(to)) = (fields.next(), fields.next()) else { break };
            out.push(Numstat { path: to.into(), old_path: Some(from.into()), added, removed, binary });
        } else {
            out.push(Numstat { path: parts[2].into(), old_path: None, added, removed, binary });
        }
    }
    out
}

pub const LARGEST: u32 = 2000;
pub const LOCK_FILES: [&str; 11] =
    ["Cargo.lock", "package-lock.json", "yarn.lock", "pnpm-lock.yaml", "Gemfile.lock", "poetry.lock", "go.sum", "Package.resolved", "composer.lock", "bun.lockb", "flake.lock"];

pub fn fold(path: &str, added: u32, removed: u32, generated: bool) -> Option<Fold> {
    if LOCK_FILES.contains(&path.rsplit('/').next().unwrap_or(path)) {
        return Some(Fold::LockFile);
    }
    if generated {
        return Some(Fold::Generated);
    }
    (added + removed > LARGEST).then_some(Fold::Large(added + removed))
}

pub fn untracked(path: &str, data: &[u8]) -> File {
    let mut file = File::new(path, Status::Added);
    file.untracked = true;
    if data[..data.len().min(8192)].contains(&0) {
        file.binary = true;
        return file;
    }
    let text = String::from_utf8_lossy(data);
    let text = text.strip_suffix('\n').unwrap_or(&text);
    if text.is_empty() {
        return file;
    }
    let lines: Vec<Line> = text.split('\n').enumerate().map(|(i, l)| Line { kind: Kind::Added, text: l.into(), old: None, new: Some(i as u32 + 1) }).collect();
    file.added = lines.len() as u32;
    file.hunks = vec![Hunk { old_start: 0, new_start: 1, scope: String::new(), lines }];
    file
}

#[cfg(test)]
mod tests {
    use super::*;

    const PATCH: &str = "diff --git a/src/pane.rs b/src/pane.rs
index 1111111..2222222 100644
--- a/src/pane.rs
+++ b/src/pane.rs
@@ -18,3 +18,4 @@ pub enum PaneType {
     Md,
     Code,
+    Diff,
 }
@@ -40,2 +41,2 @@ impl PaneType {
-    old()
+    new()
     done()
diff --git a/docs/new file.md b/docs/new file.md
new file mode 100644
index 0000000..3333333
--- /dev/null
+++ b/docs/new file.md
@@ -0,0 +1,2 @@
+# New
+text
\\ No newline at end of file
diff --git a/gone.sh b/gone.sh
deleted file mode 100755
--- a/gone.sh
+++ /dev/null
@@ -1 +0,0 @@
-echo gone
diff --git a/old.txt b/new.txt
similarity index 90%
rename from old.txt
rename to new.txt
diff --git a/logo.png b/logo.png
index 4444444..5555555 100644
Binary files a/logo.png and b/logo.png differ
";

    #[test]
    fn every_file_comes_with_its_status_and_counts() {
        let files = parse(PATCH);
        let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["src/pane.rs", "docs/new file.md", "gone.sh", "new.txt", "logo.png"]);
        let statuses: Vec<Status> = files.iter().map(|f| f.status).collect();
        assert_eq!(statuses, [Status::Modified, Status::Added, Status::Deleted, Status::Renamed, Status::Modified]);
        assert_eq!(files.iter().map(|f| (f.added, f.removed)).collect::<Vec<_>>(), [(2, 1), (2, 0), (0, 1), (0, 0), (0, 0)]);
        assert_eq!(files[3].old_path.as_deref(), Some("old.txt"));
        assert!(files[4].binary);
    }

    #[test]
    fn hunks_keep_gits_scope_and_number_both_sides() {
        let hunks = &parse(PATCH)[0].hunks;
        assert_eq!(hunks.iter().map(|h| h.scope.as_str()).collect::<Vec<_>>(), ["pub enum PaneType {", "impl PaneType {"]);
        assert_eq!(hunks[0].lines.iter().map(|l| l.kind).collect::<Vec<_>>(), [Kind::Context, Kind::Context, Kind::Added, Kind::Context]);
        assert_eq!(hunks[0].lines.iter().map(|l| l.old).collect::<Vec<_>>(), [Some(18), Some(19), None, Some(20)]);
        assert_eq!(hunks[0].lines.iter().map(|l| l.new).collect::<Vec<_>>(), [Some(18), Some(19), Some(20), Some(21)]);
        assert_eq!(hunks[1].lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(), ["    old()", "    new()", "    done()"]);
        assert_eq!(hunks[0].old_end(), 21);
    }

    #[test]
    fn quoted_paths_are_unquoted() {
        let quoted = "diff --git \"a/caf\\303\\251 x.txt\" \"b/caf\\303\\251 x.txt\"\n--- \"a/caf\\303\\251 x.txt\"\n+++ \"b/caf\\303\\251 x.txt\"\n@@ -1 +1 @@\n-a\n+b\n";
        assert_eq!(parse(quoted)[0].path, "café x.txt");
    }

    #[test]
    fn numstat_renames_take_their_two_paths() {
        let stats = numstat("3\t1\tsrc/a.rs\x000\t0\t\x00old.txt\x00new.txt\x00-\t-\tlogo.png\x00");
        assert_eq!(
            stats,
            [
                Numstat { path: "src/a.rs".into(), old_path: None, added: 3, removed: 1, binary: false },
                Numstat { path: "new.txt".into(), old_path: Some("old.txt".into()), added: 0, removed: 0, binary: false },
                Numstat { path: "logo.png".into(), old_path: None, added: 0, removed: 0, binary: true },
            ]
        );
    }

    #[test]
    fn lock_files_generated_files_and_big_diffs_fold() {
        assert_eq!(fold("Cargo.lock", 3, 0, false), Some(Fold::LockFile));
        assert_eq!(fold("web/package-lock.json", 3, 0, false), Some(Fold::LockFile));
        assert_eq!(fold("api.pb.go", 3, 0, true), Some(Fold::Generated));
        assert_eq!(fold("big.rs", 1500, 501, false), Some(Fold::Large(2001)));
        assert_eq!(fold("big.rs", 1500, 500, false), None);
        assert_eq!(Fold::Large(6240).message("huge.rs"), "6,240 changed lines: not shown.");
        assert_eq!(Fold::LockFile.message("web/Cargo.lock"), "Cargo.lock is a lock file: not shown.");
    }

    #[test]
    fn untracked_files_are_all_added() {
        let file = untracked("new.rs", b"a\nb\n");
        assert_eq!((file.status, file.untracked, file.added), (Status::Added, true, 2));
        assert_eq!(file.hunks[0].lines.iter().map(|l| l.new).collect::<Vec<_>>(), [Some(1), Some(2)]);
        assert!(untracked("x.bin", &[1, 0, 2]).binary);
    }
}
