use crate::http;
use crate::program::known_agents;
use crate::rules::{safe_name, Agent, RuleSet, Source, MAX_FILE};
use crate::version::compare_text;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::{Duration, Instant, SystemTime};

include!(concat!(env!("OUT_DIR"), "/bundled.rs"));

pub const RULES_SITE: &str = "https://jemdiggity.github.io/kanna-v4/agent-rules/index.json";
pub const CHECK_EVERY: Duration = Duration::from_secs(30 * 60);
pub const WATCH_EVERY: Duration = Duration::from_secs(2);

#[derive(Clone, Debug)]
pub struct Config {
    pub cache: PathBuf,
    pub overrides: Option<PathBuf>,
    pub site: Option<String>,
    pub check_every: Duration,
    pub watch_every: Duration,
}

impl Config {
    pub fn new(cache: impl Into<PathBuf>) -> Config {
        Config { cache: cache.into(), overrides: None, site: None, check_every: CHECK_EVERY, watch_every: WATCH_EVERY }
    }
}

pub fn bundled() -> impl Iterator<Item = (&'static str, &'static str)> {
    BUNDLED.iter().copied()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Loaded {
    pub agent: String,
    pub version: String,
    pub source: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub fetched: Vec<String>,
    pub problems: Vec<String>,
    pub loaded: Vec<Loaded>,
}

struct Inner {
    config: Config,
    current: RwLock<Arc<RuleSet>>,
    generation: AtomicU64,
    problems: Mutex<Vec<String>>,
    loaded_overrides: Mutex<Signature>,
    stop: AtomicBool,
    wake: (Mutex<bool>, Condvar),
    fetching: Mutex<()>,
}

#[derive(Clone)]
pub struct Store {
    inner: Arc<Inner>,
}

pub struct Updater {
    store: Store,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Updater {
    fn drop(&mut self) {
        self.store.inner.stop.store(true, Ordering::SeqCst);
        self.store.poke();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Store {
    pub fn new(config: Config) -> Store {
        let store = Store {
            inner: Arc::new(Inner {
                config,
                current: RwLock::new(Arc::new(RuleSet::default())),
                generation: AtomicU64::new(0),
                problems: Mutex::new(vec![]),
                loaded_overrides: Mutex::new(vec![]),
                stop: AtomicBool::new(false),
                wake: (Mutex::new(false), Condvar::new()),
                fetching: Mutex::new(()),
            }),
        };
        store.reload();
        store
    }

    pub fn rules(&self) -> Arc<RuleSet> {
        self.inner.current.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn generation(&self) -> u64 {
        self.inner.generation.load(Ordering::SeqCst)
    }

    pub fn problems(&self) -> Vec<String> {
        self.inner.problems.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn config(&self) -> &Config {
        &self.inner.config
    }

    pub fn reload(&self) {
        *self.inner.loaded_overrides.lock().unwrap_or_else(|e| e.into_inner()) = self.override_signature();
        let mut problems = vec![];
        let mut choices: BTreeMap<String, Vec<Agent>> = BTreeMap::new();
        for (name, text) in bundled() {
            match Agent::parse(text, Source::Bundled) {
                Ok(agent) if agent.name() == name => choices.entry(name.into()).or_default().push(agent),
                Ok(agent) => problems.push(format!("bundled {name}.json is for agent {}", agent.name())),
                Err(e) => problems.push(format!("bundled {name}.json: {e}")),
            }
        }
        let previous = self.rules();
        let keep = |name: &str, remote: bool, choices: &mut BTreeMap<String, Vec<Agent>>, overrides: &mut BTreeMap<String, Agent>| {
            let Some(agent) = previous.agent(name) else { return };
            match (&agent.source, remote) {
                (Source::Remote(_), true) => choices.entry(name.into()).or_default().push(agent.clone()),
                (Source::Override(_), false) => {
                    overrides.insert(name.into(), agent.clone());
                }
                _ => {}
            }
        };
        let mut overrides: BTreeMap<String, Agent> = BTreeMap::new();
        for (path, name) in rule_files(&self.remote_dir()) {
            let url = fs::read_to_string(path.with_extension("url")).unwrap_or_else(|_| path.display().to_string());
            match read_capped(&path).and_then(|t| Agent::parse(&t, Source::Remote(url.trim().into()))) {
                Ok(agent) if agent.name() == name => choices.entry(name).or_default().push(agent),
                Ok(_) => {
                    problems.push(format!("{}: the file is for another agent", path.display()));
                    keep(&name, true, &mut choices, &mut overrides);
                }
                Err(e) => {
                    problems.push(format!("{}: {e}", path.display()));
                    keep(&name, true, &mut choices, &mut overrides);
                }
            }
        }
        if let Some(dir) = &self.inner.config.overrides {
            for (path, name) in rule_files(dir) {
                let source = Source::Override(path.display().to_string());
                match read_capped(&path).and_then(|t| Agent::parse(&t, source)) {
                    Ok(agent) if agent.name() == name => {
                        overrides.insert(name, agent);
                    }
                    Ok(_) => {
                        problems.push(format!("{}: the file is for another agent (name it <agent>.json)", path.display()));
                        keep(&name, false, &mut choices, &mut overrides);
                    }
                    Err(e) => {
                        problems.push(format!("{}: {e}", path.display()));
                        keep(&name, false, &mut choices, &mut overrides);
                    }
                }
            }
        }
        let mut set = RuleSet::default();
        for (name, mut options) in choices {
            if let Some(agent) = overrides.remove(&name) {
                set.insert(agent);
                continue;
            }
            options.sort_by(|a, b| compare_text(&a.file.version, &b.file.version).then_with(|| (a.source == Source::Bundled).cmp(&(b.source == Source::Bundled))));
            if let Some(agent) = options.pop() {
                set.insert(agent);
            }
        }
        for agent in overrides.into_values() {
            set.insert(agent);
        }
        for problem in &problems {
            eprintln!("kanna-detect: rejected {problem}");
        }
        *self.inner.problems.lock().unwrap_or_else(|e| e.into_inner()) = problems;
        *self.inner.current.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(set);
        self.inner.generation.fetch_add(1, Ordering::SeqCst);
    }

    pub fn loaded(&self) -> Vec<Loaded> {
        self.rules().agents().map(|a| Loaded { agent: a.name().into(), version: a.file.version.clone(), source: a.source.label() }).collect()
    }

    pub fn update_now(&self) -> Report {
        let mut report = Report::default();
        match &self.inner.config.site {
            None => report.problems.push("no rules site is set".into()),
            Some(site) => match self.fetch(site) {
                Ok(fetched) => report.fetched = fetched,
                Err(e) => {
                    eprintln!("kanna-detect: the rules site: {e}");
                    report.problems.push(e);
                }
            },
        }
        self.reload();
        report.problems.extend(self.problems());
        report.loaded = self.loaded();
        report
    }

    pub fn start(&self) -> Updater {
        let store = self.clone();
        let thread = std::thread::Builder::new()
            .name("kanna.rules".into())
            .spawn(move || store.watch())
            .expect("start the rules updater");
        Updater { store: self.clone(), thread: Some(thread) }
    }

    fn poke(&self) {
        let (lock, condvar) = &self.inner.wake;
        *lock.lock().unwrap_or_else(|e| e.into_inner()) = true;
        condvar.notify_all();
    }

    fn watch(&self) {
        let config = &self.inner.config;
        let mut next_check = Instant::now();
        while !self.inner.stop.load(Ordering::SeqCst) {
            if config.site.is_some() && Instant::now() >= next_check {
                next_check = Instant::now() + config.check_every;
                if let Some(site) = &config.site {
                    if let Err(e) = self.fetch(site) {
                        eprintln!("kanna-detect: the rules site: {e}");
                    }
                }
                self.reload();
            }
            let loaded = self.inner.loaded_overrides.lock().unwrap_or_else(|e| e.into_inner()).clone();
            if self.override_signature() != loaded {
                self.reload();
            }
            let (lock, condvar) = &self.inner.wake;
            let guard = lock.lock().unwrap_or_else(|e| e.into_inner());
            let (mut poked, _) = condvar.wait_timeout_while(guard, config.watch_every, |poked| !*poked).unwrap_or_else(|e| e.into_inner());
            *poked = false;
        }
    }

    fn override_signature(&self) -> Signature {
        let Some(dir) = &self.inner.config.overrides else { return vec![] };
        let mut files: Vec<_> = rule_files(dir)
            .into_iter()
            .map(|(path, _)| {
                let meta = fs::metadata(&path).ok();
                (path, meta.as_ref().and_then(|m| m.modified().ok()), meta.map(|m| m.len()).unwrap_or(0))
            })
            .collect();
        files.sort();
        files
    }

    fn newest_known(&self, name: &str) -> Option<String> {
        let bundled = bundled().filter(|(n, _)| *n == name).filter_map(|(_, t)| Agent::parse(t, Source::Bundled).ok()).map(|a| a.file.version);
        let cached = read_capped(&self.remote_dir().join(format!("{name}.json"))).ok().and_then(|t| Agent::parse(&t, Source::Bundled).ok()).map(|a| a.file.version);
        bundled.chain(cached).max_by(|a, b| compare_text(a, b))
    }

    fn remote_dir(&self) -> PathBuf {
        self.inner.config.cache.join("remote")
    }

    fn fetch(&self, site: &str) -> Result<Vec<String>, String> {
        let _one = self.inner.fetching.lock().unwrap_or_else(|e| e.into_inner());
        let dir = self.remote_dir();
        fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let etag_path = dir.join("index.etag");
        let etag = fs::read_to_string(&etag_path).ok().filter(|e| !e.trim().is_empty());
        let response = http::get(site, etag.as_deref().map(str::trim))?;
        if response.status == 304 {
            return Ok(vec![]);
        }
        if response.status != 200 {
            return Err(format!("{site}: HTTP {}", response.status));
        }
        let index: Index = serde_json::from_slice(&response.body).map_err(|e| format!("{site}: not a rules index: {e}"))?;
        if index.schema != 1 {
            return Err(format!("{site}: index schema {} isn't supported", index.schema));
        }
        if index.rules.len() > MAX_ENTRIES {
            return Err(format!("{site}: over {MAX_ENTRIES} entries"));
        }
        let mut fetched = vec![];
        let mut problems = vec![];
        for entry in index.rules {
            if !index_name(&entry.agent) || !known_agents().contains(&entry.agent.as_str()) {
                problems.push(format!("{}: not an agent this Kanna knows", entry.agent.chars().take(40).collect::<String>()));
                continue;
            }
            if entry.sha256.len() != 64 || !entry.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
                problems.push(format!("{}: not a sha256", entry.agent));
                continue;
            }
            let url = http::resolve(site, &entry.url);
            if http::origin(&url).to_ascii_lowercase() != http::origin(site).to_ascii_lowercase() {
                problems.push(format!("{}: {url} isn't on the rules site", entry.agent));
                continue;
            }
            let newest = self.newest_known(&entry.agent);
            if newest.as_deref().is_some_and(|v| compare_text(&entry.version, v).is_le()) {
                continue;
            }
            match fetch_one(&url, &entry) {
                Ok(bytes) => {
                    let path = dir.join(format!("{}.json", entry.agent));
                    write_atomic(&path, &bytes).and_then(|()| write_atomic(&path.with_extension("url"), url.as_bytes())).map_err(|e| format!("{}: {e}", path.display()))?;
                    fetched.push(entry.agent);
                }
                Err(e) => problems.push(e),
            }
        }
        match response.headers.get("etag") {
            Some(etag) if problems.is_empty() => write_atomic(&etag_path, etag.as_bytes()).map_err(|e| e.to_string())?,
            _ => {
                let _ = fs::remove_file(&etag_path);
            }
        }
        for problem in &problems {
            eprintln!("kanna-detect: rejected {problem}");
        }
        if problems.is_empty() {
            Ok(fetched)
        } else {
            Err(problems.join("; "))
        }
    }
}

#[derive(Deserialize)]
struct Index {
    schema: u32,
    #[serde(default)]
    rules: Vec<Entry>,
}

#[derive(Deserialize)]
struct Entry {
    agent: String,
    url: String,
    version: String,
    sha256: String,
}

fn fetch_one(url: &str, entry: &Entry) -> Result<Vec<u8>, String> {
    let response = http::get(url, None)?;
    if response.status != 200 {
        return Err(format!("{url}: HTTP {}", response.status));
    }
    let hash = hex(&Sha256::digest(&response.body));
    if !hash.eq_ignore_ascii_case(&entry.sha256) {
        return Err(format!("{url}: its hash {hash} isn't the index's {}", entry.sha256));
    }
    let text = String::from_utf8(response.body.clone()).map_err(|_| format!("{url}: not UTF-8"))?;
    let agent = Agent::parse(&text, Source::Remote(url.into())).map_err(|e| format!("{url}: {e}"))?;
    if agent.name() != entry.agent || agent.file.version != entry.version {
        return Err(format!("{url}: the file is {} {}, the index says {} {}", agent.name(), agent.file.version, entry.agent, entry.version));
    }
    Ok(response.body)
}

const MAX_ENTRIES: usize = 64;

type Signature = Vec<(PathBuf, Option<SystemTime>, u64)>;

fn index_name(name: &str) -> bool {
    (1..=32).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn read_capped(path: &Path) -> Result<String, String> {
    let size = fs::metadata(path).map_err(|e| e.to_string())?.len();
    if size > MAX_FILE as u64 {
        return Err(format!("the file is over {} KiB", MAX_FILE / 1024));
    }
    fs::read_to_string(path).map_err(|e| e.to_string())
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn rule_files(dir: &Path) -> Vec<(PathBuf, String)> {
    let Ok(entries) = fs::read_dir(dir) else { return vec![] };
    let mut files: Vec<(PathBuf, String)> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("json") && p.is_file())
        .filter_map(|p| {
            let name = p.file_stem()?.to_str()?.to_string();
            safe_name(&name).then_some((p, name))
        })
        .filter(|(_, name)| name != "index")
        .collect();
    files.sort();
    files
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let temporary = path.with_extension(format!("tmp.{}", std::process::id()));
    fs::write(&temporary, bytes)?;
    fs::rename(&temporary, path)
}
