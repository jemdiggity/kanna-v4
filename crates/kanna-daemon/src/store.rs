use crate::reader::Feed;
use kanna_mux::json::{json, num, opt, string, Value};
use kanna_mux::PaneState;
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub const VERSION: u32 = 1;

const RECORDS: &str = "terminals.json";

const SNAPSHOT: &str = "snap";

pub const SAVE_EVERY: Duration = Duration::from_secs(1);

const TICK: Duration = Duration::from_millis(250);

pub const MAX_SNAPSHOT: usize = 16 << 20;

const MAX_RECORDS: u64 = 4 << 20;

const FLUSH_LIMIT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    pub id: String,
    pub name: Option<String>,
    pub cmd: Option<String>,
    pub cwd: Option<String>,
    pub state: PaneState,
    pub exit_code: Option<i64>,
    pub pid: i64,
    pub keeper: String,
}

#[derive(Debug, Default, PartialEq)]
pub struct Saved {
    pub counter: usize,
    pub records: Vec<Record>,
}

impl Record {
    fn to_value(&self) -> Value {
        json!({
            "id": self.id,
            "name": opt(self.name.as_deref()),
            "type": "term",
            "cmd": opt(self.cmd.as_deref()),
            "cwd": opt(self.cwd.as_deref()),
            "state": self.state.as_str(),
            "exitCode": self.exit_code.map_or(Value::Null, |c| num(c as f64)),
            "pid": num(self.pid as f64),
            "keeper": self.keeper,
        })
    }

    fn from_value(value: &Value) -> Option<Record> {
        let text = |key: &str| string(value, key).map(String::from);
        if string(value, "type") != Some("term") {
            return None;
        }
        let id = text("id").filter(|id| pane_number(id).is_some())?;
        Some(Record {
            keeper: text("keeper").unwrap_or_else(|| id.clone()),
            id,
            name: text("name").filter(|n| !n.is_empty()),
            cmd: text("cmd"),
            cwd: text("cwd"),
            state: string(value, "state").and_then(PaneState::parse)?,
            exit_code: value.get("exitCode").and_then(Value::as_f64).map(|c| c as i64),
            pid: value.get("pid").and_then(Value::as_f64)? as i64,
        })
    }
}

pub fn pane_number(id: &str) -> Option<usize> {
    let n: usize = id.strip_prefix('p')?.parse().ok()?;
    (n > 0 && n < 1_000_000_000 && format!("p{n}") == id).then_some(n)
}

pub struct Store {
    dir: PathBuf,
    saver: Sender<Job>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

enum Job {
    Track(String, Arc<Mutex<Feed>>),
    Forget(String),
    Flush(Sender<()>),
    Stop,
}

impl Store {
    pub fn open(state_dir: &Path) -> io::Result<Store> {
        let dir = state_dir.join("terminals");
        match fs::symlink_metadata(&dir) {
            Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => {
                return Err(io::Error::other(format!("{} isn't a folder", dir.display())));
            }
            Ok(_) => fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?,
            Err(_) => fs::DirBuilder::new().mode(0o700).create(&dir)?,
        }
        let (saver, jobs) = mpsc::channel();
        let folder = dir.clone();
        let thread = std::thread::Builder::new().name("kanna.save".into()).spawn(move || save_loop(&folder, jobs))?;
        Ok(Store { dir, saver, thread: Mutex::new(Some(thread)) })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn records_path(&self) -> PathBuf {
        self.dir.join(RECORDS)
    }

    pub fn load(&self) -> Saved {
        let path = self.records_path();
        let Ok(file) = fs::File::open(&path) else { return Saved::default() };
        let mut text = String::new();
        let read = io::Read::read_to_string(&mut io::Read::take(file, MAX_RECORDS + 1), &mut text);
        let parsed = read.ok().filter(|n| (*n as u64) <= MAX_RECORDS).and_then(|_| serde_json::from_str::<Value>(&text).ok());
        let saved = parsed.as_ref().and_then(|value| {
            if string(value, "kanna") != Some("terminals") || value.get("version").and_then(Value::as_f64) != Some(VERSION as f64) {
                return None;
            }
            let counter = value.get("counters").and_then(|c| c.get("pane")).and_then(Value::as_f64).unwrap_or(0.0).max(0.0) as usize;
            let mut records: Vec<Record> = Vec::new();
            for item in value.get("terminals")?.as_array()? {
                let record = Record::from_value(item)?;
                if records.iter().any(|r| r.id == record.id || r.keeper == record.keeper || (r.name.is_some() && r.name == record.name)) {
                    return None;
                }
                records.push(record);
            }
            Some(Saved { counter, records })
        });
        match saved {
            Some(saved) => saved,
            None => {
                eprintln!("kanna-daemon: {} can't be read; moved aside as {RECORDS}.bad", path.display());
                let _ = fs::rename(&path, self.dir.join(format!("{RECORDS}.bad")));
                Saved::default()
            }
        }
    }

    pub fn save(&self, saved: &Saved) -> io::Result<()> {
        let value = json!({
            "kanna": "terminals",
            "version": VERSION,
            "counters": { "pane": num(saved.counter as f64) },
            "terminals": Value::Array(saved.records.iter().map(Record::to_value).collect()),
        });
        let mut text = serde_json::to_string_pretty(&value).map_err(io::Error::other)?;
        text.push('\n');
        write_atomic(&self.records_path(), text.as_bytes())
    }

    pub fn snapshot(&self, id: &str) -> Option<Vec<u8>> {
        let path = snapshot_path(&self.dir, id);
        let size = fs::metadata(&path).ok()?.len();
        if size as usize > MAX_SNAPSHOT {
            return None;
        }
        fs::read(path).ok()
    }

    pub fn prune(&self, keep: &[String]) {
        let Ok(entries) = fs::read_dir(&self.dir) else { return };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let stale = match name.strip_suffix(&format!(".{SNAPSHOT}")) {
                Some(id) => !keep.iter().any(|k| k == id),
                None => name.contains(".tmp-"),
            };
            if stale {
                let _ = fs::remove_file(entry.path());
            }
        }
    }

    pub fn track(&self, id: &str, feed: Arc<Mutex<Feed>>) {
        let _ = self.saver.send(Job::Track(id.into(), feed));
    }

    pub fn forget(&self, id: &str) {
        let _ = self.saver.send(Job::Forget(id.into()));
    }

    pub fn flush(&self) {
        let (done, flushed) = mpsc::channel();
        if self.saver.send(Job::Flush(done)).is_ok() {
            let _ = flushed.recv_timeout(FLUSH_LIMIT);
        }
    }

    pub fn stop(&self) {
        let _ = self.saver.send(Job::Stop);
        if let Some(thread) = self.thread.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = thread.join();
        }
    }
}

fn snapshot_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.{SNAPSHOT}"))
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let temporary = path.with_file_name(format!("{name}.tmp-{}", std::process::id()));
    let _ = fs::remove_file(&temporary);
    let written = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&temporary)
        .and_then(|mut file| file.write_all(bytes))
        .and_then(|()| fs::rename(&temporary, path));
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    written
}

struct Tracked {
    feed: Arc<Mutex<Feed>>,
    next: Instant,
    failing: bool,
    too_big: bool,
}

fn save_loop(dir: &Path, jobs: mpsc::Receiver<Job>) {
    let mut tracked: HashMap<String, Tracked> = HashMap::new();
    loop {
        let job = jobs.recv_timeout(TICK);
        let mut flushed = None;
        match job {
            Ok(Job::Track(id, feed)) => {
                tracked.insert(id, Tracked { feed, next: Instant::now(), failing: false, too_big: false });
            }
            Ok(Job::Forget(id)) => {
                tracked.remove(&id);
                let _ = fs::remove_file(snapshot_path(dir, &id));
            }
            Ok(Job::Flush(done)) => flushed = Some(done),
            Ok(Job::Stop) | Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => {}
        }
        let now = Instant::now();
        for (id, terminal) in tracked.iter_mut() {
            if flushed.is_some() || terminal.next <= now {
                save_one(dir, id, terminal);
            }
        }
        if let Some(done) = flushed {
            let _ = done.send(());
        }
    }
}

fn save_one(dir: &Path, id: &str, terminal: &mut Tracked) {
    let snapshot = {
        let mut feed = Feed::lock(&terminal.feed);
        if !feed.unsaved {
            return;
        }
        let snapshot = feed.save();
        if snapshot.is_ok() {
            feed.unsaved = false;
        }
        snapshot
    };
    terminal.next = Instant::now() + SAVE_EVERY;
    let snapshot = match snapshot {
        Ok(snapshot) => snapshot,
        Err(error) => {
            if !std::mem::replace(&mut terminal.failing, true) {
                eprintln!("kanna-daemon: can't snapshot {id} for the disk yet ({error}); keeping the last one and trying again");
            }
            return;
        }
    };
    terminal.failing = false;
    if snapshot.len() > MAX_SNAPSHOT {
        if !std::mem::replace(&mut terminal.too_big, true) {
            eprintln!("kanna-daemon: {id}'s screen is {} MiB, over {} MiB; keeping the last one saved", snapshot.len() >> 20, MAX_SNAPSHOT >> 20);
        }
        return;
    }
    terminal.too_big = false;
    if let Err(error) = write_atomic(&snapshot_path(dir, id), &snapshot) {
        eprintln!("kanna-daemon: couldn't save {id}'s screen: {error}");
        Feed::lock(&terminal.feed).unsaved = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kds{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn record(id: &str, name: Option<&str>) -> Record {
        Record {
            id: id.into(),
            name: name.map(String::from),
            cmd: Some("top".into()),
            cwd: Some("/tmp".into()),
            state: PaneState::Running,
            exit_code: None,
            pid: 42,
            keeper: id.into(),
        }
    }

    #[test]
    fn records_round_trip_privately() {
        let dir = folder("records");
        let store = Store::open(&dir).unwrap();
        let mut exited = record("p3", None);
        (exited.state, exited.exit_code) = (PaneState::Exited, Some(3));
        let saved = Saved { counter: 7, records: vec![record("p1", Some("logs")), exited] };
        store.save(&saved).unwrap();
        assert_eq!(store.load(), saved);
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(store.dir()), 0o700);
        assert_eq!(mode(&store.dir().join(RECORDS)), 0o600);
        store.stop();
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_bad_file_is_moved_aside() {
        let dir = folder("bad");
        let store = Store::open(&dir).unwrap();
        let twice = Saved { counter: 2, records: vec![record("p1", Some("a")), record("p2", Some("a"))] };
        store.save(&twice).unwrap();
        assert_eq!(store.load(), Saved::default());
        assert!(store.dir().join(format!("{RECORDS}.bad")).exists());
        fs::write(store.dir().join(RECORDS), "{").unwrap();
        assert_eq!(store.load(), Saved::default());
        store.stop();
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn screens_are_saved_when_they_change_and_kept_when_they_cannot_be() {
        let dir = folder("screens");
        let store = Store::open(&dir).unwrap();
        let feed = Feed::new(20, 4).unwrap();
        Feed::lock(&feed).push(b"first");
        store.track("p1", feed.clone());
        store.flush();
        let saved = || kanna_vt::Terminal::from_snapshot(&store.snapshot("p1").unwrap()).unwrap().text(kanna_vt::Lines::Screen);
        assert_eq!(saved(), "first");
        Feed::lock(&feed).push(b" second\x1b_a\x9d");
        store.flush();
        assert_eq!(saved(), "first");
        assert!(Feed::lock(&feed).unsaved);
        Feed::lock(&feed).push(b"\x1b\\ third");
        store.flush();
        assert_eq!(saved(), "first second third");
        let mode = fs::metadata(store.dir().join("p1.snap")).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        store.forget("p1");
        store.flush();
        assert!(store.snapshot("p1").is_none());
        store.stop();
        let _ = fs::remove_dir_all(&dir);
    }
}
