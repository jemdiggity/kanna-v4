use crate::Event;
use kanna_mux::json::{json, num, opt, string, Map, Value};
use kanna_mux::{project, CoreHandle, KmuxError, PaneState, Responder, WeakCore, TASKS_PER_PROJECT};
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
    /// The worktree of a task moved back to the backlog, where it starts again.
    pub folder: Option<String>,
    /// The agent's session there, which it resumes when it starts again.
    pub session: Option<String>,
    /// The agent's pane of a task moved back to the backlog, still running, which comes back when it starts again.
    pub held: Option<String>,
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
            "folder": opt(self.folder.as_deref()),
            "session": opt(self.session.as_deref()),
            "held": opt(self.held.as_deref()),
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
            folder: text("folder"),
            session: text("session"),
            held: text("held"),
        })
    }

    /// The agent's command line. A new claude task gets `session` as its ID, so Move to Backlog can resume it;
    /// a claude task back from the backlog resumes its session, or the folder's latest one if Kanna didn't start it.
    fn command(&self, session: Option<&str>) -> String {
        let agent = in_its_terminal(&unprompted(&self.agent));
        let first = |agent: String| match &self.message {
            Some(message) => format!("{agent} {}", quote(message)),
            None => agent,
        };
        if program(&self.agent) != "claude" {
            return first(agent);
        }
        match (&self.session, &self.folder, session) {
            (Some(resume), _, _) => format!("{agent} --resume {}", quote(resume)),
            (None, Some(_), _) => format!("{agent} --continue"),
            (None, None, Some(new)) => first(format!("{agent} --session-id {new}")),
            (None, None, None) => first(agent),
        }
    }
}

/// How big the context of a claude agent in a planning terminal gets before it compacts, by default: long-lived agents do better kept small.
pub const PLANNING_CONTEXT: u64 = 300_000;

const CONTEXT_RANGE: std::ops::RangeInclusive<u64> = 100_000..=1_000_000;

/// The settings format this Kanna reads: a file without a `version` is this one, and one written for a newer Kanna is skipped.
const CONFIG_VERSION: u64 = 1;

/// Kanna's settings files for a folder, the first that sets a key winning: the repo's `.kanna/config.json`, then `~/.kanna-v4/config.json`.
pub fn config_files(folder: &Path) -> Vec<PathBuf> {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    project(folder.to_str()).map(|repo| repo.join(".kanna/config.json")).into_iter().chain([home.join(".kanna-v4/config.json")]).collect()
}

/// How big a claude agent's context gets in a planning terminal before it compacts: `planning.context`, a number of
/// tokens or `"auto"` for the agent's own, from the first of `files` that sets it.
pub fn planning_context(files: &[PathBuf]) -> Option<u64> {
    for file in files {
        let Some(text) = fs::metadata(file).ok().filter(|m| m.len() <= 64 << 10).and_then(|_| fs::read_to_string(file).ok()) else { continue };
        let value = serde_json::from_str::<Value>(&text).unwrap_or(Value::Null);
        if let Some(version) = value.get("version").and_then(Value::as_u64).filter(|v| *v > CONFIG_VERSION) {
            eprintln!("kanna-daemon: {}: settings version {version} is newer than this Kanna's ({CONFIG_VERSION}); skipping it", file.display());
            continue;
        }
        match value.get("planning").and_then(|p| p.get("context")) {
            Some(Value::String(auto)) if auto == "auto" => return None,
            Some(tokens) => match tokens.as_u64().filter(|t| CONTEXT_RANGE.contains(t)) {
                Some(tokens) => return Some(tokens),
                None => eprintln!("kanna-daemon: {}: planning.context is a number of tokens from 100000 to 1000000, or \"auto\"", file.display()),
            },
            None => {}
        }
    }
    Some(PLANNING_CONTEXT)
}

/// The planning terminal's command line: it marks the terminal as the window's planning terminal for the agent you
/// start there, keeps a claude agent's context to `context` tokens, asks about agent setup with `cli`, prints how to start,
/// and hands over to your shell.
/// Kanna's zsh integration (`zsh`, its folder) hands ZDOTDIR back to yours while the command runs, so the shell it
/// hands over to gets it again, as every Kanna terminal's does.
pub fn planner_command(guide: Option<&str>, context: Option<u64>, zsh: Option<&str>, cli: Option<&str>) -> String {
    let integrate = zsh.map_or(String::new(), |dir| format!("case \"${{SHELL##*/}}\" in zsh) [ -z \"${{ZDOTDIR+x}}\" ] || export KANNA_ZSH_ZDOTDIR=\"$ZDOTDIR\"; export ZDOTDIR={};; esac; ", quote(dir)));
    let limit = context.map_or(String::new(), |tokens| format!("[ -n \"$CLAUDE_CODE_AUTO_COMPACT_WINDOW\" ] || export CLAUDE_CODE_AUTO_COMPACT_WINDOW={tokens}; "));
    let show = guide.map_or(String::new(), |g| format!("if command -v glow >/dev/null 2>&1; then glow -w 0 {g}; else cat {g}; fi; echo; ", g = quote(g)));
    let ask = cli.map_or(String::new(), |cli| format!("{cli} agents setup --ask; {cli} agents update --ask; ", cli = quote(cli)));
    format!("export KANNA_PLANNER=1; {limit}{ask}{show}{integrate}exec \"${{SHELL:-/bin/sh}}\" -l")
}

fn cli() -> Option<PathBuf> {
    std::env::current_exe().ok()?.parent().map(|dir| dir.join("kanna")).filter(|cli| cli.is_file())
}

fn program(agent: &str) -> &str {
    let first = agent.split_whitespace().next().unwrap_or("");
    first.rsplit('/').next().unwrap_or(first)
}

const UNPROMPTED: &[(&str, &str)] = &[("claude", "--dangerously-skip-permissions"), ("codex", "--yolo")];

const ASKING: &[&str] = &["--dangerously-skip-permissions", "--yolo", "--permission-mode", "--ask-for-approval", "-a"];

fn unprompted(agent: &str) -> String {
    let agent = agent.trim();
    let program = program(agent);
    let chooses = agent.split_whitespace().skip(1).any(|w| ASKING.contains(&w.split('=').next().unwrap_or(w)));
    match UNPROMPTED.iter().find(|(name, _)| *name == program) {
        Some((_, flag)) if !chooses => format!("{agent} {flag}"),
        _ => agent.to_string(),
    }
}

fn in_its_terminal(agent: &str) -> String {
    match program(agent) == "codex" && !agent.split_whitespace().any(|w| w == "--no-daemon") {
        true => format!("{agent} --no-daemon"),
        false => agent.to_string(),
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

fn running(id: &str) -> KmuxError {
    KmuxError::new("running", format!("{id} is running: stop it with `kanna task stop {id}`"))
}

fn started_here(task: &str, pane: &str, place: &str) -> Map {
    Map::from_iter([("task".to_string(), json!(task)), ("pane".to_string(), json!(pane)), ("place".to_string(), json!(place)), ("resumed".to_string(), json!(true))])
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

/// A random (v4) UUID, in hex.
fn uuid() -> Result<String, String> {
    let mut uuid = [0u8; 16];
    std::io::Read::read_exact(&mut fs::File::open("/dev/urandom").map_err(|e| format!("can't read /dev/urandom: {e}"))?, &mut uuid).map_err(|e| format!("can't read /dev/urandom: {e}"))?;
    uuid[6] = (uuid[6] & 0x0f) | 0x40;
    uuid[8] = (uuid[8] & 0x3f) | 0x80;
    let hex: String = uuid.iter().map(|b| format!("{b:02x}")).collect();
    Ok(format!("{}-{}-{}-{}-{}", &hex[..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..]))
}

/// A worktree's name, and its branch's: `task-` and the first 32 bits of a random UUID, in hex.
fn worktree_name() -> Result<String, String> {
    Ok(format!("task-{}", &uuid()?[..8]))
}

/// Makes a worktree in `.kanna-worktrees/NAME` on a new branch NAME, and returns its folder and branch.
/// NAME is fresh for each worktree; the task's agent renames the branch once it knows what the work is.
/// In a repo with no commits yet, the branch starts empty (an orphan), so its first commit is the project's.
pub fn make_worktree(repo: &Path) -> Result<(String, String), String> {
    let git = |args: &[&str]| git(repo, args);
    git(&["rev-parse", "--show-toplevel"]).map_err(|_| format!("{} isn't a git repo", repo.display()))?;
    let born = git(&["rev-parse", "--verify", "--quiet", "HEAD"]).is_ok();
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
        let (folder, branch) = (repo.join(WORKTREES).join(&name), name);
        if folder.exists() || git(&["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")]).is_ok() {
            continue;
        }
        let folder = folder.to_string_lossy().to_string();
        if born {
            git(&["worktree", "add", "-b", &branch, &folder])?;
        } else {
            git(&["worktree", "add", "--orphan", "-b", &branch, &folder])?;
        }
        return Ok((folder, branch));
    }
    Err("every name tried was taken".into())
}

struct Starting {
    entry: Entry,
    window: Option<String>,
    backlog: bool,
    responder: Responder,
}

fn alive(core: &CoreHandle, pane: &str) -> bool {
    core.pane(pane).is_some_and(|p| matches!(p.state, PaneState::Starting | PaneState::Running))
}

fn gone() -> KmuxError {
    KmuxError::new("app_gone", "the daemon is stopping")
}

pub struct Backlog {
    path: PathBuf,
    entries: RefCell<Vec<Entry>>,
    /// The tasks this backlog started and that may still run, by their agent's pane: how to put each back.
    running: RefCell<Vec<(String, Entry)>>,
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
        let running: Vec<(String, Entry)> = saved.get("running").and_then(Value::as_array).map_or(vec![], |list| {
            list.iter().filter_map(|v| Some((string(v, "pane")?.to_string(), Entry::from_value(v)?))).collect()
        });
        let counter = saved.get("counter").and_then(Value::as_u64).unwrap_or(0) as usize;
        let counter = entries.iter().chain(running.iter().map(|(_, e)| e)).filter_map(|e| number(&e.id)).fold(counter, usize::max);
        Backlog { path, entries: RefCell::new(entries), running: RefCell::new(running), counter: Cell::new(counter), events, core, pending: RefCell::default(), next: Cell::new(0), on_change: RefCell::default() }
    }

    pub fn view(&self) -> Value {
        Value::Array(self.entries.borrow().iter().map(Entry::to_value).collect())
    }

    fn save(&self) {
        let running: Vec<Value> = self.running.borrow().iter().map(|(pane, entry)| {
            let mut value = entry.to_value();
            value["pane"] = json!(pane);
            value
        }).collect();
        let saved = json!({ "counter": self.counter.get(), "tasks": self.view(), "running": running });
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
                    Some("done") => this.remove(&args, responder),
                    Some("start") => this.start(&args, responder),
                    Some("stop") => this.stop(&args, responder),
                    Some("shelve") => this.shelve(&args, responder),
                    Some("defer") => this.defer(&args, responder),
                    Some("plan") => this.plan(&args, responder),
                    _ => responder.reply(Err(bad("backlog needs an action: new, list, done, start, stop, shelve, defer or plan"))),
                }
            }),
        );
    }

    fn mint(&self, args: &Value, name: Option<String>, message: Option<String>) -> Result<Entry, KmuxError> {
        let repo = string(args, "repo").filter(|r| Path::new(r).is_absolute() && Path::new(r).is_dir()).ok_or_else(|| bad("a task needs its repo: an absolute path to a folder"))?;
        if git(Path::new(repo), &["rev-parse", "--show-toplevel"]).is_err() {
            return Err(bad(format!("{repo} isn't a git repo: a task works in a worktree of the repo, so set one up first (git init)")));
        }
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
            folder: None,
            session: None,
            held: None,
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
        let core = self.core.upgrade();
        let tasks = self
            .entries
            .borrow()
            .iter()
            .filter(|e| repo.is_none_or(|r| e.repo == r))
            .map(|e| {
                let mut value = e.to_value();
                let pane = e.held.as_deref().zip(core.as_ref()).and_then(|(pane, core)| core.pane(pane).map(|p| (alive(core, pane), p)));
                value["running"] = json!(pane.as_ref().is_some_and(|(running, _)| *running));
                value["status"] = opt(pane.as_ref().and_then(|(_, p)| p.status()));
                value
            })
            .collect();
        Map::from_iter([("tasks".to_string(), Value::Array(tasks))])
    }

    fn take(&self, id: &str) -> Option<Entry> {
        let mut entries = self.entries.borrow_mut();
        let at = entries.iter().position(|e| e.id == id)?;
        Some(entries.remove(at))
    }

    /// Takes a task off the backlog, ending its agent and tabs if it was moved there still running.
    fn remove(&self, args: &Value, responder: Responder) {
        let Some(id) = string(args, "task") else { return responder.reply(Err(bad("say which task"))) };
        let Some(entry) = self.take(id) else { return responder.reply(Err(not_found(id))) };
        self.changed();
        let value = Map::from_iter([("task".to_string(), entry.to_value())]);
        match (entry.held, self.core.upgrade()) {
            (Some(pane), Some(core)) if core.pane(&pane).is_some() => core.handle(json!({ "cmd": "close", "args": { "task": pane } }), move |_| responder.reply(Ok(value))),
            _ => responder.reply(Ok(value)),
        }
    }

    /// Opens a window's planning terminal, in its folder (else the home folder), outside the backlog and the task limit.
    fn plan(&self, args: &Value, responder: Responder) {
        let Some(core) = self.core.upgrade() else { return responder.reply(Err(KmuxError::new("app_gone", "the daemon is stopping"))) };
        let Some(window) = string(args, "window") else { return responder.reply(Err(bad("a planning terminal needs its window"))) };
        let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
        let folder = string(args, "folder").filter(|f| Path::new(f).is_absolute() && Path::new(f).is_dir()).unwrap_or(&home);
        let zsh = self.path.parent().map(|state| state.join("shell/zsh")).filter(|dir| dir.join(".zshenv").is_file());
        let cmd = planner_command(string(args, "guide"), planning_context(&config_files(Path::new(folder))), zsh.as_deref().and_then(Path::to_str), cli().as_deref().and_then(Path::to_str));
        let open = json!({ "type": "term", "planner": true, "window": window, "cwd": folder, "cmd": cmd, "wait": false });
        core.handle(json!({ "cmd": "open", "args": open }), move |reply| match failed(&reply) {
            Some(error) => responder.reply(Err(error)),
            None => responder.reply(Ok(Map::from_iter([("pane".to_string(), reply.get("pane").and_then(|p| p.get("id")).cloned().unwrap_or(Value::Null)), ("window".to_string(), reply.get("window").cloned().unwrap_or(Value::Null))]))),
        });
    }

    fn shelve(self: &Rc<Self>, args: &Value, responder: Responder) {
        let Some(id) = string(args, "task") else { return responder.reply(Err(bad("say which task"))) };
        let Some(entry) = self.entries.borrow().iter().find(|e| e.id == id).cloned() else { return responder.reply(Err(not_found(id))) };
        if self.pending.borrow().values().any(|p| p.entry.id == entry.id) {
            return responder.reply(Err(bad(format!("{id} is starting"))));
        }
        self.room(entry, string(args, "window").map(String::from), responder);
    }

    fn start(self: &Rc<Self>, args: &Value, responder: Responder) {
        let Some(core) = self.core.upgrade() else { return responder.reply(Err(gone())) };
        let Some(reference) = string(args, "task") else { return responder.reply(Err(bad("say which task"))) };
        let found = self.entries.borrow().iter().find(|e| e.id == reference).cloned();
        if let Some(entry) = found {
            if self.pending.borrow().values().any(|p| p.entry.id == entry.id) {
                return responder.reply(Err(bad(format!("{reference} is starting"))));
            }
            let id = entry.id.clone();
            return match entry.held.clone().filter(|pane| core.pane(pane).is_some()) {
                Some(pane) if alive(&core, &pane) => responder.reply(Err(running(&id))),
                Some(pane) => self.revive(&pane.clone(), Some(entry.command(None)), move |result| responder.reply(result.map(|_| started_here(&id, &pane, "backlog")))),
                None => self.prepare(Entry { held: None, ..entry }, None, true, responder),
            };
        }
        let this = Rc::downgrade(self);
        self.task_pane(reference, move |found| {
            let Some(this) = this.upgrade() else { return };
            let pane = match found {
                Ok(pane) => pane,
                Err(error) => return responder.reply(Err(error)),
            };
            if this.core.upgrade().is_some_and(|core| alive(&core, &pane)) {
                return responder.reply(Err(running(&pane)));
            }
            let entry = this.running.borrow().iter().find(|(p, _)| *p == pane).map(|(_, e)| e.clone());
            let id = pane.clone();
            this.revive(&pane, entry.map(|e| e.command(None)), move |result| responder.reply(result.map(|_| started_here(&id, &id, "shelf"))));
        });
    }

    fn stop(self: &Rc<Self>, args: &Value, responder: Responder) {
        let Some(reference) = string(args, "task") else { return responder.reply(Err(bad("say which task"))) };
        let found = self.entries.borrow().iter().find(|e| e.id == reference).cloned();
        let this = Rc::downgrade(self);
        let end = move |task: String, pane: String, place: &'static str, responder: Responder| {
            let Some(core) = this.upgrade().and_then(|this| this.core.upgrade()) else { return responder.reply(Err(gone())) };
            if !alive(&core, &pane) {
                return responder.reply(Err(KmuxError::new("stopped", format!("{task} is stopped: start it with `kanna task start {task}`"))));
            }
            let weak = core.weak();
            core.handle(json!({ "cmd": "close", "args": { "task": pane, "tabs": true } }), move |closed| {
                if let Some(error) = failed(&closed) {
                    return responder.reply(Err(error));
                }
                let Some(core) = weak.upgrade() else { return responder.reply(Err(gone())) };
                let out = Map::from_iter([("task".to_string(), json!(task)), ("pane".to_string(), json!(pane)), ("place".to_string(), json!(place))]);
                core.end(&pane, move |ended| responder.reply(ended.map(|_| out)));
            });
        };
        if let Some(entry) = found {
            let pane = entry.held.clone().filter(|pane| self.core.upgrade().is_some_and(|core| core.pane(pane).is_some()));
            return match pane {
                Some(pane) => end(entry.id, pane, "backlog", responder),
                None => responder.reply(Err(KmuxError::new("stopped", format!("{} is stopped: start it with `kanna task start {}`", entry.id, entry.id)))),
            };
        }
        self.task_pane(reference, move |found| match found {
            Ok(pane) => end(pane.clone(), pane, "shelf", responder),
            Err(error) => responder.reply(Err(error)),
        });
    }

    fn task_pane(&self, reference: &str, then: impl FnOnce(Result<String, KmuxError>) + 'static) {
        let Some(core) = self.core.upgrade() else { return then(Err(gone())) };
        let Some(pane) = core.pane(reference).map(|p| p.id) else { return then(Err(KmuxError::new("not_found", format!("no task \"{reference}\"")))) };
        let reference = reference.to_string();
        core.handle(json!({ "cmd": "task", "args": { "action": "list" } }), move |reply| {
            if let Some(error) = failed(&reply) {
                return then(Err(error));
            }
            let listed = reply.get("tasks").and_then(Value::as_array).is_some_and(|tasks| tasks.iter().any(|t| string(t, "id") == Some(pane.as_str())));
            then(if listed { Ok(pane) } else { Err(KmuxError::new("not_a_task", format!("{reference} isn't a task: a task is an agent docked on a Desk or the Shelf, or in the backlog"))) })
        });
    }

    fn revive(&self, pane: &str, command: Option<String>, done: impl FnOnce(Result<(), KmuxError>) + 'static) {
        let Some(core) = self.core.upgrade() else { return done(Err(gone())) };
        if let Some(command) = command {
            core.with_pane_mut(pane, |p| p.command = Some(command));
        }
        core.handle(json!({ "cmd": "restart", "args": { "pane": pane } }), move |reply| done(failed(&reply).map_or(Ok(()), Err)));
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
                return responder.reply(Err(KmuxError::new("task_limit", format!("{} has {TASKS_PER_PROJECT} tasks on the Shelf: move one to the backlog with `kanna task defer TASK` first", entry.repo))));
            }
            this.prepare(entry, window, false, responder);
        });
    }

    fn prepare(self: &Rc<Self>, mut entry: Entry, window: Option<String>, backlog: bool, responder: Responder) {
        if let Some(pane) = entry.held.clone() {
            return self.release(pane, entry, window, responder);
        }
        if let Some(folder) = entry.folder.clone() {
            if Path::new(&folder).is_dir() {
                let branch = git(Path::new(&folder), &["branch", "--show-current"]).unwrap_or_default();
                return self.launch(Starting { entry, window, backlog, responder }, folder, branch);
            }
            (entry.folder, entry.session) = (None, None);
        }
        let token = self.next.get() + 1;
        self.next.set(token);
        let (repo, events) = (PathBuf::from(&entry.repo), self.events.clone());
        self.pending.borrow_mut().insert(token, Starting { entry, window, backlog, responder });
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

    /// Remembers how to put a running task back on the backlog, forgetting tasks whose agent is gone.
    fn record(&self, pane: &str, entry: Entry) {
        let core = self.core.upgrade();
        let mut running = self.running.borrow_mut();
        running.retain(|(p, _)| p != pane && core.as_ref().is_none_or(|core| core.pane(p).is_some()));
        running.push((pane.to_string(), entry));
    }

    /// Move to Backlog: takes a running task off its Desk or the Shelf and puts it back on its repo's backlog,
    /// its agent and tabs still running, so starting it again brings them back as they are.
    fn defer(self: &Rc<Self>, args: &Value, responder: Responder) {
        let Some(core) = self.core.upgrade() else { return responder.reply(Err(KmuxError::new("app_gone", "the daemon is stopping"))) };
        let Some(reference) = string(args, "task") else { return responder.reply(Err(bad("say which running task"))) };
        let Some(pane) = core.pane(reference) else { return responder.reply(Err(KmuxError::new("not_found", format!("no running task \"{reference}\"")))) };
        let recorded = self.running.borrow().iter().find(|(p, _)| *p == pane.id).map(|(_, e)| e.clone());
        let entry = match recorded {
            Some(entry) => entry,
            None => {
                let Some(folder) = pane.opened_in.clone().or(pane.cwd.clone()) else { return responder.reply(Err(bad(format!("{} has no folder to come back to", pane.id)))) };
                let repo = project(Some(&folder)).map(|r| r.to_string_lossy().to_string()).unwrap_or_default();
                let agent = pane.command.as_deref().map(program).filter(|p| !p.is_empty()).map(String::from);
                let args = json!({ "repo": repo, "agent": opt(agent.as_deref()) });
                match self.mint(&args, pane.name.clone(), None) {
                    Ok(entry) => Entry { folder: Some(folder), ..entry },
                    Err(error) => return responder.reply(Err(error)),
                }
            }
        };
        let this = Rc::downgrade(self);
        let id = pane.id.clone();
        core.handle(json!({ "cmd": "task", "args": { "action": "hold", "task": id } }), move |reply| {
            let Some(this) = this.upgrade() else { return };
            if let Some(error) = failed(&reply) {
                return responder.reply(Err(error));
            }
            this.running.borrow_mut().retain(|(p, _)| *p != id);
            let entry = Entry { held: Some(id.clone()), ..entry };
            let value = entry.to_value();
            this.entries.borrow_mut().push(entry);
            this.changed();
            let running = this.core.upgrade().is_some_and(|core| alive(&core, &id));
            responder.reply(Ok(Map::from_iter([("task".to_string(), value), ("pane".to_string(), json!(id)), ("running".to_string(), json!(running))])));
        });
    }

    fn launch(self: &Rc<Self>, starting: Starting, folder: String, branch: String) {
        let Some(core) = self.core.upgrade() else { return starting.responder.reply(Err(KmuxError::new("app_gone", "the daemon is stopping"))) };
        let Starting { entry, window, backlog, responder } = starting;
        let name = entry.name.clone().filter(|n| core.pane(n).is_none());
        let session = (program(&entry.agent) == "claude" && entry.folder.is_none()).then(uuid).and_then(Result::ok);
        let place = if backlog { "backlog" } else { "shelf" };
        let mut args = json!({ "type": "term", "dock": "left", place: true, "cwd": folder, "cmd": entry.command(session.as_deref()), "wait": false });
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
            let mut ran = entry.clone();
            ran.folder = Some(folder.clone());
            ran.session = ran.session.or(session);
            if backlog {
                return this.running_here(pane, ran, branch, responder);
            }
            this.started(pane, ran, branch, window, &weak, responder);
        });
    }

    /// Brings a task held in the backlog back to the Shelf, its agent and tabs as they were, or starts it again
    /// in its worktree when its agent's pane is gone.
    fn release(self: &Rc<Self>, pane: String, entry: Entry, window: Option<String>, responder: Responder) {
        let Some(core) = self.core.upgrade() else { return responder.reply(Err(KmuxError::new("app_gone", "the daemon is stopping"))) };
        let this = Rc::downgrade(self);
        let weak = self.core.clone();
        core.handle(json!({ "cmd": "task", "args": { "action": "release", "task": pane } }), move |reply| {
            let Some(this) = this.upgrade() else { return };
            if let Some(error) = failed(&reply) {
                if weak.upgrade().is_some_and(|core| core.pane(&pane).is_some()) {
                    return responder.reply(Err(error));
                }
                return this.prepare(Entry { held: None, ..entry }, window, false, responder);
            }
            let folder = entry.folder.clone().unwrap_or_default();
            let branch = git(Path::new(&folder), &["branch", "--show-current"]).unwrap_or_default();
            let entry = Entry { held: None, ..entry };
            if weak.upgrade().is_some_and(|core| alive(&core, &pane)) {
                return this.started(pane, entry, branch, window, &weak, responder);
            }
            let again = Rc::downgrade(&this);
            this.revive(&pane.clone(), Some(entry.command(None)), move |result| {
                let Some(this) = again.upgrade() else { return };
                match result {
                    Ok(()) => this.started(pane, entry, branch, window, &weak, responder),
                    Err(error) => responder.reply(Err(error)),
                }
            });
        });
    }

    fn running_here(&self, pane: String, ran: Entry, branch: String, responder: Responder) {
        let out = Map::from_iter([
            ("task".to_string(), json!(ran.id)),
            ("pane".to_string(), json!(pane)),
            ("place".to_string(), json!("backlog")),
            ("folder".to_string(), json!(ran.folder)),
            ("branch".to_string(), json!(branch)),
        ]);
        if let Some(entry) = self.entries.borrow_mut().iter_mut().find(|e| e.id == ran.id) {
            *entry = Entry { held: Some(pane), ..ran };
        }
        self.changed();
        responder.reply(Ok(out));
    }

    /// A task now runs on the Shelf as `pane`: it leaves the backlog, and goes onto `window`'s Desk if it was asked for.
    fn started(&self, pane: String, ran: Entry, branch: String, window: Option<String>, core: &WeakCore, responder: Responder) {
        self.take(&ran.id);
        let mut out = Map::from_iter([
            ("task".to_string(), json!(ran.id)),
            ("pane".to_string(), json!(pane)),
            ("folder".to_string(), json!(ran.folder)),
            ("branch".to_string(), json!(branch)),
        ]);
        self.record(&pane, ran);
        self.changed();
        let (Some(window), Some(core)) = (window, core.upgrade()) else { return responder.reply(Ok(out)) };
        core.handle(json!({ "cmd": "task", "args": { "action": "pull", "task": pane, "window": window } }), move |pulled| {
            out.insert("window".into(), pulled.get("window").cloned().unwrap_or(Value::Null));
            responder.reply(Ok(out));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_first_message_is_quoted_for_the_shell() {
        let mut entry = Entry { id: "t1".into(), name: None, message: Some("don't stop".into()), repo: "/r".into(), by: None, made_at: 0.0, agent: "codex".into(), folder: None, session: None, held: None };
        assert_eq!(entry.command(None), "codex --yolo --no-daemon 'don'\\''t stop'");
        entry.message = None;
        assert_eq!(entry.command(None), "codex --yolo --no-daemon");
    }

    #[test]
    fn a_claude_task_gets_a_session_and_resumes_it_when_it_comes_back_from_the_backlog() {
        let mut entry = Entry { id: "t1".into(), name: None, message: Some("go".into()), repo: "/r".into(), by: None, made_at: 0.0, agent: "/bin/claude".into(), folder: None, session: None, held: None };
        let id = uuid().unwrap();
        assert!(id.len() == 36 && id.chars().nth(14) == Some('4'), "{id}");
        assert_eq!(entry.command(Some(&id)), format!("/bin/claude --dangerously-skip-permissions --session-id {id} 'go'"));
        entry.folder = Some("/r/.kanna-worktrees/task-1".into());
        assert_eq!(entry.command(None), "/bin/claude --dangerously-skip-permissions --continue", "a task Kanna didn't start resumes the folder's latest session");
        entry.session = Some(id.clone());
        assert_eq!(entry.command(None), format!("/bin/claude --dangerously-skip-permissions --resume '{id}'"));
        entry.agent = "codex".into();
        assert_eq!(entry.command(None), "codex --yolo --no-daemon 'go'", "only claude resumes for now");
    }

    #[test]
    fn the_planning_terminal_is_marked_keeps_claude_to_its_context_shows_the_guide_and_is_your_shell() {
        let command = planner_command(Some("/K/getting started.md"), Some(300_000), None, None);
        assert_eq!(command, "export KANNA_PLANNER=1; [ -n \"$CLAUDE_CODE_AUTO_COMPACT_WINDOW\" ] || export CLAUDE_CODE_AUTO_COMPACT_WINDOW=300000; if command -v glow >/dev/null 2>&1; then glow -w 0 '/K/getting started.md'; else cat '/K/getting started.md'; fi; echo; exec \"${SHELL:-/bin/sh}\" -l");
        assert_eq!(planner_command(None, None, None, None), "export KANNA_PLANNER=1; exec \"${SHELL:-/bin/sh}\" -l");
        assert_eq!(planner_command(None, None, None, Some("/A/kanna")), "export KANNA_PLANNER=1; '/A/kanna' agents setup --ask; '/A/kanna' agents update --ask; exec \"${SHELL:-/bin/sh}\" -l");
        let zsh = planner_command(None, None, Some("/S/shell/zsh"), None);
        assert!(zsh.ends_with("case \"${SHELL##*/}\" in zsh) [ -z \"${ZDOTDIR+x}\" ] || export KANNA_ZSH_ZDOTDIR=\"$ZDOTDIR\"; export ZDOTDIR='/S/shell/zsh';; esac; exec \"${SHELL:-/bin/sh}\" -l"), "{zsh}");
        let run = |extra: &[(&str, &str)]| {
            let mut sh = Command::new("/bin/sh");
            sh.arg("-c").arg(planner_command(Some("/dev/null"), Some(300_000), None, None).replace("exec \"${SHELL:-/bin/sh}\" -l", "echo \"P=$KANNA_PLANNER C=$CLAUDE_CODE_AUTO_COMPACT_WINDOW\"")).env_remove("CLAUDE_CODE_AUTO_COMPACT_WINDOW");
            sh.envs(extra.iter().copied());
            String::from_utf8_lossy(&sh.output().unwrap().stdout).trim().to_string()
        };
        assert_eq!(run(&[]), "P=1 C=300000");
        assert_eq!(run(&[("CLAUDE_CODE_AUTO_COMPACT_WINDOW", "500000")]), "P=1 C=500000", "your own setting wins");
    }

    #[test]
    fn the_settings_schema_agents_read_matches_what_the_daemon_reads() {
        let schema: Value = serde_json::from_str(include_str!("../../../schemas/config.schema.json")).unwrap();
        let context = &schema["properties"]["planning"]["properties"]["context"];
        assert_eq!(context["default"], json!(PLANNING_CONTEXT));
        assert_eq!((context["oneOf"][0]["minimum"].clone(), context["oneOf"][0]["maximum"].clone()), (json!(CONTEXT_RANGE.start()), json!(CONTEXT_RANGE.end())));
        assert_eq!(context["oneOf"][1]["const"], "auto");
        assert_eq!(schema["properties"]["version"]["const"], json!(CONFIG_VERSION));
        assert!(schema["$id"].as_str().unwrap().ends_with(&format!("/v{CONFIG_VERSION}.schema.json")));
        let keys: Vec<&String> = schema["properties"]["planning"]["properties"].as_object().unwrap().keys().collect();
        assert_eq!(keys, ["context"], "a key in the schema the daemon doesn't read, or the other way round");
    }

    #[test]
    fn the_planning_context_comes_from_the_repos_config_then_yours() {
        let dir = std::env::temp_dir().join(format!("kanna-planning-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("repo/.kanna")).unwrap();
        fs::create_dir_all(dir.join("home/.kanna-v4")).unwrap();
        let (repo, mine) = (dir.join("repo/.kanna/config.json"), dir.join("home/.kanna-v4/config.json"));
        let files = [repo.clone(), mine.clone()];
        assert_eq!(planning_context(&files), Some(PLANNING_CONTEXT), "no files: 300K");
        fs::write(&mine, r#"{"planning": {"context": 500000}}"#).unwrap();
        assert_eq!(planning_context(&files), Some(500_000));
        fs::write(&repo, r#"{"planning": {"context": "auto"}}"#).unwrap();
        assert_eq!(planning_context(&files), None, "the repo's wins");
        fs::write(&repo, r#"{"other": 1}"#).unwrap();
        assert_eq!(planning_context(&files), Some(500_000), "a file that doesn't set it leaves it to the next");
        fs::write(&repo, r#"{"version": 2, "planning": {"context": "auto"}}"#).unwrap();
        assert_eq!(planning_context(&files), Some(500_000), "a file for a newer Kanna is skipped");
        fs::write(&repo, r#"{"version": 1, "planning": {"context": "auto"}}"#).unwrap();
        assert_eq!(planning_context(&files), None);
        for (text, context) in [(r#"{"planning": {"context": 50}}"#, Some(PLANNING_CONTEXT)), ("not json", Some(PLANNING_CONTEXT))] {
            fs::write(&mine, text).unwrap();
            assert_eq!(planning_context(&[mine.clone()]), context, "{text}");
        }
        assert!(Command::new("git").arg("-C").arg(dir.join("repo")).args(["init", "-q"]).status().unwrap().success());
        let found = config_files(&dir.join("repo"));
        assert_eq!(found[0].canonicalize().ok(), repo.canonicalize().ok(), "{found:?}");
        assert!(found[1].ends_with(".kanna-v4/config.json"), "{found:?}");
        fs::remove_dir_all(&dir).unwrap();
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
    fn codex_runs_in_its_terminal_not_the_shared_server() {
        assert_eq!(in_its_terminal("/opt/bin/codex --yolo"), "/opt/bin/codex --yolo --no-daemon");
        assert_eq!(in_its_terminal("codex --no-daemon --yolo"), "codex --no-daemon --yolo");
        assert_eq!(in_its_terminal("claude"), "claude");
    }

    #[test]
    fn entries_round_trip_and_ids_are_t_numbers() {
        let entry = Entry { id: "t7".into(), name: Some("Fix it".into()), message: None, repo: "/r".into(), by: Some("p2".into()), made_at: 5.0, agent: "codex".into(), folder: Some("/r/.kanna-worktrees/task-1".into()), session: Some("s".into()), held: Some("p3".into()) };
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
        let hex = name.strip_prefix("task-").unwrap_or_else(|| panic!("{name}"));
        assert!(hex.len() == 8 && hex.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()), "{name}");
        assert_eq!(folder, dir.join(".kanna-worktrees").join(&name).to_string_lossy());
        assert_eq!(branch, name);
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
        let backlog = Backlog::open(&state, core.weak(), std::sync::mpsc::channel().0);
        let ran = Entry { id: "t9".into(), name: Some("Ran".into()), message: None, repo: "/r".into(), by: None, made_at: 0.0, agent: "claude".into(), folder: Some("/r/w".into()), session: Some("s".into()), held: None };
        backlog.record("p4", ran.clone());
        backlog.save();
        let again = Backlog::open(&state, core.weak(), std::sync::mpsc::channel().0);
        assert_eq!(*again.running.borrow(), vec![("p4".to_string(), ran)], "a running task's way back survives a restart");
        assert_eq!(again.mint(&args, None, None).unwrap().id, "t10", "and its ID isn't handed out again");
        fs::remove_dir_all(&state).unwrap();
    }

    #[test]
    fn a_folder_that_isnt_a_repo_is_left_alone_and_a_new_repos_first_task_starts_on_an_empty_branch() {
        let dir = std::env::temp_dir().join(format!("kanna-empty-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let dir = dir.canonicalize().unwrap();
        assert!(make_worktree(&dir).unwrap_err().contains("isn't a git repo"));
        assert!(!dir.join(".git").exists(), "Kanna made a repo");
        assert!(Command::new("git").arg("-C").arg(&dir).args(["init", "-q"]).status().unwrap().success());
        let (folder, branch) = make_worktree(&dir).unwrap();
        let run = |at: &str, args: &[&str]| Command::new("git").arg("-C").arg(at).args(args).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&run(&folder, &["branch", "--show-current"]).stdout).trim(), branch);
        assert!(!run(&folder, &["rev-parse", "--verify", "HEAD"]).status.success(), "a commit was made for the user");
        assert_eq!(String::from_utf8_lossy(&run(dir.to_str().unwrap(), &["status", "--porcelain"]).stdout), "");
        let (other, _) = make_worktree(&dir).unwrap();
        assert_ne!(other, folder);
        fs::remove_dir_all(&dir).unwrap();
    }
}
