use crate::Event;
use kanna_mux::json::{json, num, opt, string, Map, Value};
use kanna_mux::{project, CoreHandle, KmuxError, Responder, WeakCore, TASKS_PER_PROJECT};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::sync::mpsc::Sender;
use std::time::{SystemTime, UNIX_EPOCH};

const FILE: &str = "backlog.json";

const WORKTREES: &str = ".kanna-worktrees";

const MAX_FILE: u64 = 4 << 20;

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub id: String,
    pub name: Option<String>,
    pub message: Option<String>,
    pub repo: String,
    pub by: Option<String>,
    pub made_at: f64,
    pub agent: String,
}

impl Entry {
    fn to_value(&self) -> Value {
        json!({
            "id": self.id,
            "name": opt(self.name.as_deref()),
            "message": opt(self.message.as_deref()),
            "repo": self.repo,
            "by": opt(self.by.as_deref()),
            "madeAt": num(self.made_at),
            "agent": self.agent,
        })
    }

    fn from_value(value: &Value) -> Option<Entry> {
        let text = |key: &str| string(value, key).filter(|s| !s.is_empty()).map(String::from);
        Some(Entry {
            id: text("id").filter(|id| number(id).is_some())?,
            name: text("name"),
            message: text("message"),
            repo: text("repo")?,
            by: text("by"),
            made_at: value.get("madeAt").and_then(Value::as_f64).unwrap_or(0.0),
            agent: text("agent").unwrap_or_else(default_agent),
        })
    }

    fn command(&self) -> String {
        let agent = unprompted(&self.agent);
        match &self.message {
            Some(message) => format!("{agent} {}", quote(message)),
            None => agent,
        }
    }
}

const UNPROMPTED: &[(&str, &str)] = &[("claude", "--dangerously-skip-permissions"), ("codex", "--yolo")];

const ASKING: &[&str] = &["--dangerously-skip-permissions", "--yolo", "--permission-mode", "--ask-for-approval", "-a"];

fn unprompted(agent: &str) -> String {
    let agent = agent.trim();
    let mut words = agent.split_whitespace();
    let program = words.next().map(|w| w.rsplit('/').next().unwrap_or(w)).unwrap_or("");
    let chooses = words.any(|w| ASKING.contains(&w.split('=').next().unwrap_or(w)));
    match UNPROMPTED.iter().find(|(name, _)| *name == program) {
        Some((_, flag)) if !chooses => format!("{agent} {flag}"),
        _ => agent.to_string(),
    }
}

fn default_agent() -> String {
    std::env::var("KANNA_AGENT").ok().filter(|a| !a.trim().is_empty()).unwrap_or_else(|| "claude".into())
}

fn number(id: &str) -> Option<usize> {
    id.strip_prefix('t').filter(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())).and_then(|n| n.parse().ok())
}

fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64().floor())
}

fn bad(message: impl Into<String>) -> KmuxError {
    KmuxError::new("bad_request", message)
}

fn not_found(id: &str) -> KmuxError {
    KmuxError::new("not_found", format!("no task \"{id}\" in the backlog"))
}

fn failed(value: &Value) -> Option<KmuxError> {
    if value.get("ok") == Some(&Value::Bool(true)) {
        return None;
    }
    let error = value.get("error").cloned().unwrap_or(Value::Null);
    Some(KmuxError::new(string(&error, "code").unwrap_or("failed"), string(&error, "message").unwrap_or("it failed")))
}

#[cfg(target_os = "macos")]
pub fn process_cwd(pid: i64) -> Option<String> {
    let mut info: libc::proc_vnodepathinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_vnodepathinfo>() as libc::c_int;
    let got = unsafe { libc::proc_pidinfo(pid as libc::c_int, libc::PROC_PIDVNODEPATHINFO, 0, (&mut info as *mut libc::proc_vnodepathinfo).cast(), size) };
    if got != size {
        return None;
    }
    let bytes: Vec<u8> = info.pvi_cdir.vip_path.iter().flatten().map(|&c| c as u8).take_while(|&b| b != 0).collect();
    Some(String::from_utf8_lossy(&bytes).into_owned()).filter(|p| !p.is_empty())
}

#[cfg(not(target_os = "macos"))]
pub fn process_cwd(pid: i64) -> Option<String> {
    fs::read_link(format!("/proc/{pid}/cwd")).ok().map(|p| p.to_string_lossy().into_owned())
}

fn git(repo: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .args(["-c", "core.hooksPath=/dev/null", "-c", "core.fsmonitor=false"])
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map_err(|e| format!("can't run git: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// A worktree's name: the first 32 bits of a random (v4) UUID, in hex.
fn worktree_name() -> Result<String, String> {
    let mut uuid = [0u8; 16];
    std::io::Read::read_exact(&mut fs::File::open("/dev/urandom").map_err(|e| format!("can't read /dev/urandom: {e}"))?, &mut uuid).map_err(|e| format!("can't read /dev/urandom: {e}"))?;
    uuid[6] = (uuid[6] & 0x0f) | 0x40;
    uuid[8] = (uuid[8] & 0x3f) | 0x80;
    Ok(uuid[..4].iter().map(|b| format!("{b:02x}")).collect())
}

/// Makes a worktree in `.kanna-worktrees/NAME` on a new branch `kanna/NAME`, and returns its folder and branch.
/// NAME is fresh for each worktree; the task's agent renames the branch once it knows what the work is.
pub fn make_worktree(repo: &Path) -> Result<(String, String), String> {
    let git = |args: &[&str]| git(repo, args);
    let common = git(&["rev-parse", "--git-common-dir"])?;
    let exclude = repo.join(common).join("info/exclude");
    let listed = fs::read_to_string(&exclude).unwrap_or_default();
    let line = format!("/{WORKTREES}/");
    if !listed.lines().any(|l| l.trim() == line) {
        let _ = fs::create_dir_all(exclude.parent().unwrap_or(repo));
        let text = if listed.is_empty() || listed.ends_with('\n') { format!("{listed}{line}\n") } else { format!("{listed}\n{line}\n") };
        fs::write(&exclude, text).map_err(|e| format!("can't write {}: {e}", exclude.display()))?;
    }
    for _ in 0..8 {
        let name = worktree_name()?;
        let (folder, branch) = (repo.join(WORKTREES).join(&name), format!("kanna/{name}"));
        if folder.exists() || git(&["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")]).is_ok() {
            continue;
        }
        let folder = folder.to_string_lossy().to_string();
        git(&["worktree", "add", "-b", &branch, &folder])?;
        return Ok((folder, branch));
    }
    Err("every name tried was taken".into())
}

struct Starting {
    entry: Entry,
    window: Option<String>,
    responder: Responder,
}

pub struct Backlog {
    path: PathBuf,
    entries: RefCell<Vec<Entry>>,
    counter: Cell<usize>,
    events: Sender<Event>,
    core: WeakCore,
    pending: RefCell<HashMap<u64, Starting>>,
    next: Cell<u64>,
    pub on_change: RefCell<Option<Box<dyn Fn(Value)>>>,
}

impl Backlog {
    pub fn open(state_dir: &Path, core: WeakCore, events: Sender<Event>) -> Backlog {
        let path = state_dir.join(FILE);
        let saved = fs::metadata(&path)
            .ok()
            .filter(|m| m.len() <= MAX_FILE)
            .and_then(|_| fs::read_to_string(&path).ok())
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .unwrap_or(Value::Null);
        let entries: Vec<Entry> = saved.get("tasks").and_then(Value::as_array).map_or(vec![], |list| list.iter().filter_map(Entry::from_value).collect());
        let counter = saved.get("counter").and_then(Value::as_u64).unwrap_or(0) as usize;
        let counter = entries.iter().filter_map(|e| number(&e.id)).fold(counter, usize::max);
        Backlog { path, entries: RefCell::new(entries), counter: Cell::new(counter), events, core, pending: RefCell::default(), next: Cell::new(0), on_change: RefCell::default() }
    }

    pub fn view(&self) -> Value {
        Value::Array(self.entries.borrow().iter().map(Entry::to_value).collect())
    }

    fn save(&self) {
        let saved = json!({ "counter": self.counter.get(), "tasks": self.view() });
        let temporary = self.path.with_extension("json.tmp");
        let written = fs::write(&temporary, saved.to_string()).and_then(|_| fs::rename(&temporary, &self.path));
        if let Err(error) = written {
            eprintln!("kanna-daemon: couldn't save the backlog to {}: {error}", self.path.display());
        }
    }

    fn changed(&self) {
        self.save();
        if let Some(on_change) = self.on_change.borrow().as_ref() {
            on_change(self.view());
        }
    }

    pub fn install(self: &Rc<Self>, core: &CoreHandle) {
        let this = Rc::downgrade(self);
        core.add_command(
            "backlog",
            Rc::new(move |args, responder| {
                let Some(this) = this.upgrade() else { return responder.reply(Err(KmuxError::new("app_gone", "the daemon is stopping"))) };
                match args.get("action").and_then(Value::as_str) {
                    Some("new") => responder.reply(this.add(&args)),
                    Some("list") => responder.reply(Ok(this.list(&args))),
                    Some("done") => responder.reply(this.remove(&args)),
                    Some("start") => this.start(&args, responder),
                    Some("blank") => this.blank(&args, responder),
                    _ => responder.reply(Err(bad("backlog needs an action: new, list, done, start or blank"))),
                }
            }),
        );
    }

    fn mint(&self, args: &Value, name: Option<String>, message: Option<String>) -> Result<Entry, KmuxError> {
        let repo = string(args, "repo").filter(|r| Path::new(r).is_absolute() && Path::new(r).is_dir()).ok_or_else(|| bad("a task needs its repo: an absolute path to a folder"))?;
        git(Path::new(repo), &["rev-parse", "--git-dir"]).map_err(|_| bad(format!("{repo} isn't a git repo: a task works in a worktree of its own")))?;
        let by = string(args, "by").and_then(|by| self.core.upgrade()?.pane(by)).map(|pane| pane.id);
        self.counter.set(self.counter.get() + 1);
        self.save();
        Ok(Entry {
            id: format!("t{}", self.counter.get()),
            name,
            message,
            repo: repo.to_string(),
            by,
            made_at: now(),
            agent: string(args, "agent").filter(|a| !a.trim().is_empty()).map(String::from).unwrap_or_else(default_agent),
        })
    }

    fn add(&self, args: &Value) -> Result<Map, KmuxError> {
        let name = string(args, "name").map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad("a task needs a name"))?.to_string();
        let message = string(args, "message").map(str::trim).filter(|m| !m.is_empty()).map(String::from);
        let entry = self.mint(args, Some(name), message)?;
        let value = entry.to_value();
        self.entries.borrow_mut().push(entry);
        self.changed();
        Ok(Map::from_iter([("task".to_string(), value)]))
    }

    fn list(&self, args: &Value) -> Map {
        let repo = string(args, "repo");
        let tasks = self.entries.borrow().iter().filter(|e| repo.is_none_or(|r| e.repo == r)).map(Entry::to_value).collect();
        Map::from_iter([("tasks".to_string(), Value::Array(tasks))])
    }

    fn take(&self, id: &str) -> Option<Entry> {
        let mut entries = self.entries.borrow_mut();
        let at = entries.iter().position(|e| e.id == id)?;
        Some(entries.remove(at))
    }

    fn remove(&self, args: &Value) -> Result<Map, KmuxError> {
        let id = string(args, "task").ok_or_else(|| bad("say which task"))?;
        let entry = self.take(id).ok_or_else(|| not_found(id))?;
        self.changed();
        Ok(Map::from_iter([("task".to_string(), entry.to_value())]))
    }

    fn blank(self: &Rc<Self>, args: &Value, responder: Responder) {
        let here = project(string(args, "repo"));
        let real = self.core.upgrade().is_some_and(|core| {
            core.with_panes(|panes| panes.iter().filter_map(|p| p.pid).filter_map(process_cwd).any(|cwd| project(Some(&cwd)) == here))
        });
        if !real {
            return responder.reply(Err(bad("Start a task works in a repo a Kanna terminal is in: cd there first")));
        }
        match self.mint(args, None, None) {
            Ok(entry) => self.room(entry, string(args, "window").map(String::from), responder),
            Err(error) => responder.reply(Err(error)),
        }
    }

    fn start(self: &Rc<Self>, args: &Value, responder: Responder) {
        let Some(id) = string(args, "task") else { return responder.reply(Err(bad("say which task"))) };
        let Some(entry) = self.entries.borrow().iter().find(|e| e.id == id).cloned() else { return responder.reply(Err(not_found(id))) };
        if self.pending.borrow().values().any(|p| p.entry.id == entry.id) {
            return responder.reply(Err(bad(format!("{id} is starting"))));
        }
        self.room(entry, string(args, "window").map(String::from), responder);
    }

    fn room(self: &Rc<Self>, entry: Entry, window: Option<String>, responder: Responder) {
        let Some(core) = self.core.upgrade() else { return responder.reply(Err(KmuxError::new("app_gone", "the daemon is stopping"))) };
        let this = Rc::downgrade(self);
        let weak = self.core.clone();
        core.handle(json!({ "cmd": "task", "args": { "action": "list" } }), move |reply| {
            let Some(this) = this.upgrade() else { return };
            if let Some(error) = failed(&reply) {
                return responder.reply(Err(error));
            }
            let here = project(Some(&entry.repo));
            let running = weak.upgrade().map_or(0, |core| {
                let ids: Vec<String> = reply.get("tasks").and_then(Value::as_array).map_or(vec![], |l| l.iter().filter_map(|t| string(t, "id").map(String::from)).collect());
                core.with_panes(|panes| ids.iter().filter_map(|id| panes.get(id)).filter(|p| project(p.cwd.as_deref()) == here).count())
            });
            if running >= TASKS_PER_PROJECT {
                return responder.reply(Err(KmuxError::new("task_limit", format!("{} has {TASKS_PER_PROJECT} tasks running: end one with `kanna task done TASK` first", entry.repo))));
            }
            this.prepare(entry, window, responder);
        });
    }

    fn prepare(self: &Rc<Self>, entry: Entry, window: Option<String>, responder: Responder) {
        let token = self.next.get() + 1;
        self.next.set(token);
        let (repo, events) = (PathBuf::from(&entry.repo), self.events.clone());
        self.pending.borrow_mut().insert(token, Starting { entry, window, responder });
        std::thread::spawn(move || {
            let result = make_worktree(&repo);
            let _ = events.send(Event::Worktree { token, result });
        });
    }

    pub fn worktree_made(self: &Rc<Self>, token: u64, result: Result<(String, String), String>) {
        let Some(starting) = self.pending.borrow_mut().remove(&token) else { return };
        match result {
            Ok((folder, branch)) => self.launch(starting, folder, branch),
            Err(error) => starting.responder.reply(Err(KmuxError::new("failed", format!("couldn't make {}'s worktree: {error}", starting.entry.id)))),
        }
    }

    fn launch(self: &Rc<Self>, starting: Starting, folder: String, branch: String) {
        let Some(core) = self.core.upgrade() else { return starting.responder.reply(Err(KmuxError::new("app_gone", "the daemon is stopping"))) };
        let Starting { entry, window, responder } = starting;
        let name = entry.name.clone().filter(|n| core.pane(n).is_none());
        let mut args = json!({ "type": "term", "dock": "left", "shelf": true, "cwd": folder, "cmd": entry.command(), "wait": false });
        if let Some(name) = &name {
            args["name"] = json!(name);
        }
        let this = Rc::downgrade(self);
        let weak = self.core.clone();
        core.handle(json!({ "cmd": "open", "args": args }), move |reply| {
            let Some(this) = this.upgrade() else { return };
            if let Some(error) = failed(&reply) {
                return responder.reply(Err(error));
            }
            let pane = reply.get("pane").and_then(|p| string(p, "id")).unwrap_or("").to_string();
            if this.take(&entry.id).is_some() {
                this.changed();
            }
            let mut out = Map::from_iter([
                ("task".to_string(), json!(entry.id)),
                ("pane".to_string(), json!(pane)),
                ("folder".to_string(), json!(folder)),
                ("branch".to_string(), json!(branch)),
            ]);
            let (Some(window), Some(core)) = (window, weak.upgrade()) else { return responder.reply(Ok(out)) };
            core.handle(json!({ "cmd": "task", "args": { "action": "pull", "task": pane, "window": window } }), move |pulled| {
                out.insert("window".into(), pulled.get("window").cloned().unwrap_or(Value::Null));
                responder.reply(Ok(out));
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_process_folder_comes_from_the_system() {
        let here = std::env::current_dir().unwrap().canonicalize().unwrap();
        let seen = process_cwd(std::process::id() as i64).map(|p| PathBuf::from(p).canonicalize().unwrap());
        assert_eq!(seen, Some(here));
        assert_eq!(process_cwd(i32::MAX as i64), None);
    }

    #[test]
    fn a_first_message_is_quoted_for_the_shell() {
        let mut entry = Entry { id: "t1".into(), name: None, message: Some("don't stop".into()), repo: "/r".into(), by: None, made_at: 0.0, agent: "claude".into() };
        assert_eq!(entry.command(), "claude --dangerously-skip-permissions 'don'\\''t stop'");
        entry.message = None;
        assert_eq!(entry.command(), "claude --dangerously-skip-permissions");
    }

    #[test]
    fn agents_start_without_asking_for_permission_unless_told_how_to_ask() {
        assert_eq!(unprompted("codex"), "codex --yolo");
        assert_eq!(unprompted("/opt/bin/claude --model opus"), "/opt/bin/claude --model opus --dangerously-skip-permissions");
        assert_eq!(unprompted("claude --dangerously-skip-permissions"), "claude --dangerously-skip-permissions");
        assert_eq!(unprompted("claude --permission-mode plan"), "claude --permission-mode plan");
        assert_eq!(unprompted("codex --ask-for-approval=untrusted"), "codex --ask-for-approval=untrusted");
        assert_eq!(unprompted("codex -a untrusted"), "codex -a untrusted");
        assert_eq!(unprompted("opencode"), "opencode");
    }

    #[test]
    fn entries_round_trip_and_ids_are_t_numbers() {
        let entry = Entry { id: "t7".into(), name: Some("Fix it".into()), message: None, repo: "/r".into(), by: Some("p2".into()), made_at: 5.0, agent: "codex".into() };
        assert_eq!(Entry::from_value(&entry.to_value()), Some(entry));
        assert_eq!(number("t12"), Some(12));
        assert_eq!(number("p3"), None);
        assert_eq!(number("t"), None);
    }

    #[test]
    fn a_worktree_gets_its_own_branch_and_is_kept_out_of_git_status() {
        let dir = std::env::temp_dir().join(format!("kanna-backlog-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let git = |args: &[&str]| assert!(Command::new("git").arg("-C").arg(&dir).args(args).output().unwrap().status.success(), "git {args:?}");
        git(&["init", "-q", "-b", "main"]);
        git(&["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-q", "--allow-empty", "-m", "first"]);
        let (folder, branch) = make_worktree(&dir).unwrap();
        let name = folder.rsplit('/').next().unwrap().to_string();
        assert!(name.len() == 8 && name.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()), "{name}");
        assert_eq!(folder, dir.join(".kanna-worktrees").join(&name).to_string_lossy());
        assert_eq!(branch, format!("kanna/{name}"));
        let current = Command::new("git").arg("-C").arg(&folder).args(["branch", "--show-current"]).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&current.stdout).trim(), branch);
        let canonical = |p: PathBuf| p.canonicalize().unwrap();
        assert_eq!(project(Some(&format!("{folder}/src"))).map(canonical), Some(canonical(dir.clone())));
        assert_eq!(project(Some(dir.to_str().unwrap())).map(canonical), Some(canonical(dir.clone())));
        let status = Command::new("git").arg("-C").arg(&dir).args(["status", "--porcelain"]).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&status.stdout), "");
        let hook = dir.join(".git/hooks/post-checkout");
        let marker = dir.join("hooked");
        fs::write(&hook, format!("#!/bin/sh\ntouch '{}'\n", marker.display())).unwrap();
        fs::set_permissions(&hook, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let (other, _) = make_worktree(&dir).unwrap();
        assert_ne!(other, folder);
        assert!(!marker.exists(), "a repo's hooks ran");
        let exclude = fs::read_to_string(dir.join(".git/info/exclude")).unwrap();
        assert_eq!(exclude.matches("/.kanna-worktrees/").count(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_id_is_never_handed_out_twice_across_a_restart() {
        let state = std::env::temp_dir().join(format!("kanna-ids-{}", std::process::id()));
        let _ = fs::remove_dir_all(&state);
        fs::create_dir_all(&state).unwrap();
        assert!(Command::new("git").arg("-C").arg(&state).args(["init", "-q"]).status().unwrap().success());
        let core = kanna_mux::Core::default();
        let args = json!({ "repo": state.to_str().unwrap() });
        let mint = || Backlog::open(&state, core.weak(), std::sync::mpsc::channel().0).mint(&args, None, None).unwrap().id;
        assert_eq!(mint(), "t1");
        assert_eq!(mint(), "t2");
        fs::remove_dir_all(&state).unwrap();
    }
}
