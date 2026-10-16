pub mod patch;

use patch::{File, Status};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

pub const PINNED: [&str; 6] = ["-c", "core.quotepath=false", "-c", "diff.noprefix=false", "-c", "diff.mnemonicPrefix=false"];
pub const FORMAT: [&str; 5] = ["--no-color", "--no-ext-diff", "--no-textconv", "--src-prefix=a/", "--dst-prefix=b/"];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Diff {
    pub folder: String,
    pub args: Vec<String>,
    pub top: String,
    pub git_dir: String,
    pub common_dir: String,
    pub files: Vec<File>,
    pub error: Option<String>,
}

impl Diff {
    pub fn command(&self) -> String {
        std::iter::once("git diff").chain(self.args.iter().map(String::as_str)).collect::<Vec<_>>().join(" ")
    }

    pub fn to_json(&self) -> Value {
        let added: u32 = self.files.iter().map(|f| f.added).sum();
        let removed: u32 = self.files.iter().map(|f| f.removed).sum();
        json!({
            "folder": self.folder, "args": self.args, "command": self.command(), "top": self.top,
            "files": self.files.iter().map(File::to_json).collect::<Vec<_>>(),
            "added": added, "removed": removed, "error": self.error,
        })
    }
}

pub fn git(args: &[&str], folder: &str, input: Option<&[u8]>) -> Result<Vec<u8>, String> {
    let mut command = Command::new("git");
    command
        .args(PINNED)
        .args(args)
        .current_dir(folder)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_PAGER", "cat")
        .env("LC_ALL", "C")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|e| format!("can't run git in {folder}: {e}"))?;
    if let (Some(input), Some(mut stdin)) = (input, child.stdin.take()) {
        let input = input.to_vec();
        std::thread::spawn(move || {
            let _ = stdin.write_all(&input);
        });
    }
    let out = child.wait_with_output().map_err(|e| format!("git failed: {e}"))?;
    if !out.status.success() {
        let message = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if message.is_empty() { format!("git exited with {}", out.status) } else { message });
    }
    Ok(out.stdout)
}

fn text(args: &[&str], folder: &str) -> Result<String, String> {
    git(args, folder, None).map(|out| String::from_utf8_lossy(&out).into_owned())
}

pub fn options(args: &[String]) -> &[String] {
    match args.iter().position(|a| a == "--") {
        Some(i) => &args[..i],
        None => args,
    }
}

pub fn pathspecs(args: &[String]) -> &[String] {
    match args.iter().position(|a| a == "--") {
        Some(i) => &args[i + 1..],
        None => &[],
    }
}

pub fn shows_worktree(args: &[String], folder: &str) -> bool {
    let options = options(args);
    if options.iter().any(|a| a == "--staged" || a == "--cached" || a == "--no-index") {
        return false;
    }
    let mut query = vec!["rev-parse", "--revs-only", "--no-flags"];
    query.extend(options.iter().map(String::as_str));
    text(&query, folder).map(|out| out.lines().filter(|l| !l.is_empty()).count() <= 1).unwrap_or(true)
}

fn diff_args<'a>(extra: &[&'a str], args: &'a [String]) -> Vec<&'a str> {
    let mut out = vec!["diff"];
    out.extend(FORMAT);
    out.extend(extra);
    out.extend(args.iter().map(String::as_str));
    out
}

pub fn load(folder: &str, args: &[String], show: &HashSet<String>) -> Diff {
    let mut diff = Diff { folder: folder.into(), args: args.to_vec(), ..Default::default() };
    if let Err(error) = fill(&mut diff, show) {
        diff.error = Some(error);
        diff.files.clear();
    }
    diff
}

fn fill(diff: &mut Diff, show: &HashSet<String>) -> Result<(), String> {
    let folder = diff.folder.clone();
    let args = diff.args.clone();
    let dirs = text(&["rev-parse", "--path-format=absolute", "--show-toplevel", "--git-dir", "--git-common-dir"], &folder)?;
    let mut dirs = dirs.lines();
    diff.top = dirs.next().unwrap_or_default().into();
    diff.git_dir = dirs.next().unwrap_or_default().into();
    diff.common_dir = dirs.next().unwrap_or_default().into();
    let stats = patch::numstat(&text(&diff_args(&["--numstat", "-z"], &args), &folder)?);
    let generated = generated(&stats.iter().map(|s| s.path.as_str()).collect::<Vec<_>>(), &diff.top)?;
    let mut folds = BTreeMap::new();
    for stat in stats.iter().filter(|s| !s.binary && !show.contains(&s.path)) {
        if let Some(fold) = patch::fold(&stat.path, stat.added, stat.removed, generated.contains(&stat.path)) {
            folds.insert(stat.path.clone(), fold);
        }
    }
    let mut patch_args = args.clone();
    if !folds.is_empty() {
        if !patch_args.iter().any(|a| a == "--") {
            patch_args.push("--".into());
        }
        if pathspecs(&patch_args).is_empty() {
            patch_args.push(":(top)".into());
        }
        patch_args.extend(folds.keys().map(|p| format!(":(top,exclude,literal){p}")));
    }
    let parsed = patch::parse(&text(&diff_args(&[], &patch_args), &folder)?);
    let mut by_path: HashMap<String, File> = parsed.into_iter().map(|f| (f.path.clone(), f)).collect();
    for stat in &stats {
        let mut file = match by_path.remove(&stat.path) {
            Some(file) => file,
            None => {
                let mut file = File::new(&stat.path, Status::Modified);
                if let Some(fold) = folds.get(&stat.path) {
                    file.fold = Some(fold.clone());
                    file.status = status(&stat.path, &args, &folder);
                }
                file
            }
        };
        file.added = stat.added;
        file.removed = stat.removed;
        file.binary |= stat.binary;
        if file.old_path.is_none() {
            file.old_path = stat.old_path.clone();
        }
        if file.old_path.is_some() && file.status == Status::Modified {
            file.status = Status::Renamed;
        }
        diff.files.push(file);
    }
    if shows_worktree(&args, &folder) {
        let mut query = vec!["ls-files", "--others", "--exclude-standard", "--full-name", "-z", "--"];
        query.extend(pathspecs(&args).iter().map(String::as_str));
        for path in text(&query, &folder)?.split('\0').filter(|p| !p.is_empty()) {
            let data = std::fs::read(Path::new(&diff.top).join(path)).unwrap_or_default();
            let mut file = patch::untracked(path, &data);
            if !show.contains(path) && !file.binary {
                if let Some(fold) = patch::fold(path, file.added, 0, false) {
                    file.fold = Some(fold);
                    file.hunks.clear();
                }
            }
            diff.files.push(file);
        }
    }
    Ok(())
}

fn status(path: &str, args: &[String], folder: &str) -> Status {
    let literal = format!(":(top,literal){path}");
    let mut query = diff_args(&["--name-status", "-z"], options(args));
    query.extend(["--", literal.as_str()]);
    match text(&query, folder).ok().and_then(|out| out.chars().next()) {
        Some('A') => Status::Added,
        Some('D') => Status::Deleted,
        Some('R') => Status::Renamed,
        _ => Status::Modified,
    }
}

fn generated(paths: &[&str], top: &str) -> Result<HashSet<String>, String> {
    if paths.is_empty() {
        return Ok(HashSet::new());
    }
    let mut input = paths.join("\0").into_bytes();
    input.push(0);
    let out = git(&["check-attr", "-z", "--stdin", "linguist-generated"], top, Some(&input))?;
    let out = String::from_utf8_lossy(&out);
    let fields: Vec<&str> = out.split('\0').collect();
    Ok(fields.chunks(3).filter(|c| c.len() == 3 && (c[2] == "true" || c[2] == "set")).map(|c| c[0].to_string()).collect())
}

pub fn ignored(paths: &[String], top: &str) -> HashSet<String> {
    if paths.is_empty() {
        return HashSet::new();
    }
    let mut input = paths.join("\0").into_bytes();
    input.push(0);
    match git(&["check-ignore", "-z", "--stdin"], top, Some(&input)) {
        Ok(out) => String::from_utf8_lossy(&out).split('\0').filter(|p| !p.is_empty()).map(String::from).collect(),
        Err(_) => HashSet::new(),
    }
}
