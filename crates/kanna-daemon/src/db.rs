use kanna_mux::json::Value;
use rusqlite::{params, Connection, OptionalExtension};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const FILE: &str = "kanna.db";

const BACKUP: &str = "kanna.db.bak";

const BACKLOG: &str = "backlog.json";

const IMPORTED: &str = "backlog.json.imported";

const MAX_BACKLOG: u64 = 4 << 20;

const MIGRATIONS: &[&str] = &["
    create table meta (key text primary key, value text not null);
    create table tasks (
        id text primary key,
        repo text not null,
        place text not null check (place in ('backlog', 'shelf', 'desk', 'done')),
        pane text,
        made_at real not null,
        doc text not null default '{}' check (json_valid(doc))
    );
    create index tasks_by_place on tasks (place, repo);
    create table task_events (
        id integer primary key,
        task text not null references tasks (id),
        at real not null,
        what text not null,
        from_place text,
        to_place text,
        by text,
        doc text not null default '{}' check (json_valid(doc))
    );
    create index task_events_by_task on task_events (task, at);
    create index task_events_by_time on task_events (at);
"];

pub fn version() -> i64 {
    MIGRATIONS.len() as i64
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Backlog,
    Shelf,
    Desk,
    Done,
}

impl Place {
    pub fn as_str(self) -> &'static str {
        match self {
            Place::Backlog => "backlog",
            Place::Shelf => "shelf",
            Place::Desk => "desk",
            Place::Done => "done",
        }
    }

    fn parse(text: &str) -> Option<Place> {
        [Place::Backlog, Place::Shelf, Place::Desk, Place::Done].into_iter().find(|p| p.as_str() == text)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub id: String,
    pub repo: String,
    pub place: Place,
    pub pane: Option<String>,
    pub made_at: f64,
    pub doc: Value,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    pub what: &'static str,
    pub from: Option<Place>,
    pub to: Option<Place>,
    pub by: Option<String>,
    pub at: Option<f64>,
    pub doc: Value,
}

impl Change {
    pub fn new(what: &'static str) -> Change {
        Change { what, from: None, to: None, by: None, at: None, doc: Value::Object(Default::default()) }
    }

    pub fn moved(mut self, from: Place, to: Place) -> Change {
        (self.from, self.to) = (Some(from), Some(to));
        self
    }

    pub fn by(mut self, by: Option<&str>) -> Change {
        self.by = by.map(String::from);
        self
    }

    pub fn with(mut self, key: &str, value: Value) -> Change {
        self.doc[key] = value;
        self
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Logged {
    pub task: String,
    pub at: f64,
    pub what: String,
    pub from: Option<Place>,
    pub to: Option<Place>,
    pub by: Option<String>,
    pub doc: Value,
}

pub struct Db {
    conn: Connection,
}

fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

fn stamp() -> String {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()).to_string()
}

fn say(message: String) {
    eprintln!("kanna-daemon: {message}");
}

fn aside(path: &Path, suffix: &str) -> PathBuf {
    let to = path.with_file_name(format!("{}.{suffix}", path.file_name().and_then(|n| n.to_str()).unwrap_or(FILE)));
    for extra in ["-wal", "-shm"] {
        let from = PathBuf::from(format!("{}{extra}", path.display()));
        if from.exists() {
            let _ = fs::rename(&from, PathBuf::from(format!("{}{extra}", to.display())));
        }
    }
    let _ = fs::rename(path, &to);
    to
}

fn connect(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    conn.busy_timeout(Duration::from_secs(5))?;
    conn.pragma_update(None, "journal_mode", "wal")?;
    conn.pragma_update(None, "synchronous", "normal")?;
    conn.pragma_update(None, "foreign_keys", "on")?;
    Ok(conn)
}

enum Opened {
    Good(Db),
    Newer(i64),
    Bad(String),
}

fn open_checked(path: &Path) -> Opened {
    let conn = match connect(path) {
        Ok(conn) => conn,
        Err(error) => return Opened::Bad(error.to_string()),
    };
    let check: rusqlite::Result<String> = conn.query_row("pragma quick_check", [], |row| row.get(0));
    match check {
        Ok(ok) if ok == "ok" => {}
        Ok(problem) => return Opened::Bad(problem),
        Err(error) => return Opened::Bad(error.to_string()),
    }
    let found: i64 = match conn.query_row("pragma user_version", [], |row| row.get(0)) {
        Ok(found) => found,
        Err(error) => return Opened::Bad(error.to_string()),
    };
    if found > version() {
        return Opened::Newer(found);
    }
    let mut db = Db { conn };
    match db.migrate(found) {
        Ok(()) => Opened::Good(db),
        Err(error) => Opened::Bad(format!("migrating from version {found}: {error}")),
    }
}

impl Db {
    pub fn open(state_dir: &Path) -> Db {
        let path = state_dir.join(FILE);
        let backup = state_dir.join(BACKUP);
        let mut lost = false;
        let mut db = None;
        if path.exists() {
            match open_checked(&path) {
                Opened::Good(good) => db = Some(good),
                Opened::Newer(found) => {
                    let to = aside(&path, "newer");
                    say(format!("{} is from a newer Kanna (version {found}, this one reads {}); moved it to {} and left it alone", path.display(), version(), to.display()));
                    lost = true;
                }
                Opened::Bad(why) => {
                    let to = aside(&path, &format!("bad-{}", stamp()));
                    say(format!("{} can't be used ({why}); moved it to {}", path.display(), to.display()));
                    lost = true;
                }
            }
        } else if backup.exists() || state_dir.join(IMPORTED).exists() {
            say(format!("{} is missing", path.display()));
            lost = true;
        }
        if db.is_none() && backup.exists() {
            let restored = fs::copy(&backup, &path).map_err(|e| e.to_string()).map(|_| open_checked(&path));
            match restored {
                Ok(Opened::Good(good)) => {
                    say(format!("restored {} from {}, the copy made when the daemon last started; changes since then are lost", path.display(), backup.display()));
                    db = Some(good);
                }
                Ok(Opened::Newer(_)) | Ok(Opened::Bad(_)) | Err(_) => {
                    aside(&path, &format!("bad-{}", stamp()));
                    say(format!("{} can't be used either", backup.display()));
                }
            }
        }
        let db = match db {
            Some(db) => db,
            None => Db::fresh(&path, state_dir, lost),
        };
        db.absorb(state_dir);
        if let Err(error) = db.conn.backup(rusqlite::MAIN_DB, &backup, None) {
            say(format!("couldn't copy {} to {}: {error}", path.display(), backup.display()));
        } else {
            let _ = fs::set_permissions(&backup, fs::Permissions::from_mode(0o600));
        }
        db
    }

    fn fresh(path: &Path, state_dir: &Path, lost: bool) -> Db {
        let mut db = match connect(path) {
            Ok(conn) => Db { conn },
            Err(error) => {
                say(format!("can't make {} ({error}); keeping tasks in memory only until the daemon restarts", path.display()));
                Db { conn: Connection::open_in_memory().expect("an in-memory database") }
            }
        };
        db.migrate(0).expect("a new database takes the schema");
        let imported = state_dir.join(IMPORTED);
        if lost && !state_dir.join(BACKLOG).exists() {
            match imported.exists() {
                true => match read_backlog(&imported).and_then(|saved| db.import(&saved).map_err(|e| e.to_string())) {
                    Ok(count) => say(format!("started a new {} from {} ({count} tasks); tasks made or changed since it was imported are lost", path.display(), imported.display())),
                    Err(error) => say(format!("started a new, empty {}: {} can't be read ({error})", path.display(), imported.display())),
                },
                false => say(format!("started a new, empty {}: there's no copy of the tasks to restore", path.display())),
            }
        }
        db
    }

    fn absorb(&self, state_dir: &Path) {
        let file = state_dir.join(BACKLOG);
        if !file.exists() {
            return;
        }
        let imported = match read_backlog(&file).and_then(|saved| self.import(&saved).map_err(|e| e.to_string())) {
            Ok(count) => count,
            Err(error) => return say(format!("can't import {} ({error}); left it where it is", file.display())),
        };
        let to = state_dir.join(IMPORTED);
        match fs::rename(&file, &to) {
            Ok(()) => say(format!("imported {imported} tasks from {} into {FILE}; the file is now {}", file.display(), to.display())),
            Err(error) => say(format!("imported {imported} tasks from {} into {FILE}, but can't rename it ({error})", file.display())),
        }
    }

    fn migrate(&mut self, from: i64) -> rusqlite::Result<()> {
        for (step, sql) in MIGRATIONS.iter().enumerate().skip(from as usize) {
            let tx = self.conn.transaction()?;
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", step as i64 + 1)?;
            tx.commit()?;
        }
        Ok(())
    }

    pub fn import(&self, saved: &Value) -> rusqlite::Result<usize> {
        let tx = self.conn.unchecked_transaction()?;
        let mut count = 0;
        let mut highest = 0;
        let tasks = saved.get("tasks").and_then(Value::as_array).into_iter().flatten().map(|t| (t, Place::Backlog, t.get("held")));
        let running = saved.get("running").and_then(Value::as_array).into_iter().flatten().map(|t| (t, Place::Shelf, t.get("pane")));
        for (task, place, pane) in tasks.chain(running) {
            let Some(row) = row_from_saved(task, place, pane.and_then(Value::as_str)) else { continue };
            highest = highest.max(number(&row.id).unwrap_or(0));
            let added = tx.execute(
                "insert or ignore into tasks (id, repo, place, pane, made_at, doc) values (?1, ?2, ?3, ?4, ?5, ?6)",
                params![row.id, row.repo, row.place.as_str(), row.pane, row.made_at, row.doc.to_string()],
            )?;
            if added == 1 {
                let made = Change { at: Some(row.made_at), by: row.doc.get("by").and_then(Value::as_str).map(String::from), to: Some(Place::Backlog), ..Change::new("made") };
                log(&tx, &row.id, &made)?;
                if place == Place::Shelf {
                    log(&tx, &row.id, &Change::new("imported").moved(Place::Backlog, Place::Shelf).by(Some("daemon")))?;
                }
                count += 1;
            }
        }
        let counter = saved.get("counter").and_then(Value::as_u64).unwrap_or(0).max(highest);
        let current: u64 = tx.query_row("select value from meta where key = 'next_task'", [], |r| r.get::<_, String>(0)).optional()?.and_then(|v| v.parse().ok()).unwrap_or(0);
        tx.execute("insert or replace into meta (key, value) values ('next_task', ?1)", [current.max(counter).to_string()])?;
        tx.commit()?;
        Ok(count)
    }

    pub fn tasks(&self) -> rusqlite::Result<Vec<Row>> {
        let mut statement = self.conn.prepare("select id, repo, place, pane, made_at, doc from tasks where place != 'done' order by made_at, rowid")?;
        let rows = statement.query_map([], |r| {
            let place: String = r.get(2)?;
            let doc: String = r.get(5)?;
            Ok(Row { id: r.get(0)?, repo: r.get(1)?, place: Place::parse(&place).unwrap_or(Place::Backlog), pane: r.get(3)?, made_at: r.get(4)?, doc: serde_json::from_str(&doc).unwrap_or(Value::Null) })
        })?;
        rows.collect()
    }

    pub fn counter(&self) -> rusqlite::Result<u64> {
        let value: Option<String> = self.conn.query_row("select value from meta where key = 'next_task'", [], |r| r.get(0)).optional()?;
        Ok(value.and_then(|v| v.parse().ok()).unwrap_or(0))
    }

    pub fn next_task(&self) -> rusqlite::Result<String> {
        let tx = self.conn.unchecked_transaction()?;
        let next = tx.query_row("select coalesce((select cast(value as integer) from meta where key = 'next_task'), 0) + 1", [], |r| r.get::<_, i64>(0))?;
        tx.execute("insert or replace into meta (key, value) values ('next_task', ?1)", [next.to_string()])?;
        tx.commit()?;
        Ok(format!("t{next}"))
    }

    pub fn put(&self, row: &Row, change: Option<&Change>) -> rusqlite::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "insert into tasks (id, repo, place, pane, made_at, doc) values (?1, ?2, ?3, ?4, ?5, json_patch('{}', ?6))
             on conflict (id) do update set repo = excluded.repo, place = excluded.place, pane = excluded.pane, made_at = excluded.made_at, doc = json_patch(tasks.doc, ?6)",
            params![row.id, row.repo, row.place.as_str(), row.pane, row.made_at, row.doc.to_string()],
        )?;
        if let Some(change) = change {
            log(&tx, &row.id, change)?;
        }
        tx.commit()
    }

    pub fn log(&self, task: &str, change: &Change) -> rusqlite::Result<()> {
        log(&self.conn, task, change)
    }

    pub fn events(&self, task: &str) -> rusqlite::Result<Vec<Logged>> {
        let mut statement = self.conn.prepare("select task, at, what, from_place, to_place, by, doc from task_events where task = ?1 order by at, id")?;
        let rows = statement.query_map([task], |r| {
            let place = |i: usize| r.get::<_, Option<String>>(i).map(|p| p.as_deref().and_then(Place::parse));
            let doc: String = r.get(6)?;
            Ok(Logged { task: r.get(0)?, at: r.get(1)?, what: r.get(2)?, from: place(3)?, to: place(4)?, by: r.get(5)?, doc: serde_json::from_str(&doc).unwrap_or(Value::Null) })
        })?;
        rows.collect()
    }

    pub fn place(&self, task: &str) -> rusqlite::Result<Option<Place>> {
        let place: Option<String> = self.conn.query_row("select place from tasks where id = ?1", [task], |r| r.get(0)).optional()?;
        Ok(place.as_deref().and_then(Place::parse))
    }
}

fn log(conn: &Connection, task: &str, change: &Change) -> rusqlite::Result<()> {
    conn.execute(
        "insert into task_events (task, at, what, from_place, to_place, by, doc) values (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![task, change.at.unwrap_or_else(now), change.what, change.from.map(Place::as_str), change.to.map(Place::as_str), change.by, change.doc.to_string()],
    )?;
    Ok(())
}

fn number(id: &str) -> Option<u64> {
    id.strip_prefix('t').filter(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())).and_then(|n| n.parse().ok())
}

fn read_backlog(file: &Path) -> Result<Value, String> {
    let size = fs::metadata(file).map_err(|e| e.to_string())?.len();
    if size > MAX_BACKLOG {
        return Err(format!("it's {size} bytes, more than a backlog holds"));
    }
    let text = fs::read_to_string(file).map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

fn row_from_saved(task: &Value, place: Place, pane: Option<&str>) -> Option<Row> {
    let text = |key: &str| task.get(key).and_then(Value::as_str).filter(|s| !s.is_empty()).map(String::from);
    let id = text("id").filter(|id| number(id).is_some())?;
    let repo = text("repo")?;
    let mut doc = serde_json::Map::new();
    for key in ["name", "message", "by", "agent", "folder", "session"] {
        if let Some(value) = text(key) {
            doc.insert(key.into(), Value::String(value));
        }
    }
    Some(Row { id, repo, place, pane: pane.filter(|p| !p.is_empty()).map(String::from), made_at: task.get("madeAt").and_then(Value::as_f64).unwrap_or(0.0), doc: Value::Object(doc) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanna_mux::json::json;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kanna-db-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn real() -> Value {
        serde_json::from_str(include_str!("../tests/fixtures/backlog.json")).unwrap()
    }

    #[test]
    fn a_real_backlog_json_moves_into_the_db_with_no_task_lost() {
        let dir = folder("import");
        let saved = real();
        fs::write(dir.join(BACKLOG), saved.to_string()).unwrap();
        let db = Db::open(&dir);
        assert!(!dir.join(BACKLOG).exists() && dir.join(IMPORTED).exists());
        let rows = db.tasks().unwrap();
        let waiting = saved["tasks"].as_array().unwrap();
        let running = saved["running"].as_array().unwrap();
        assert_eq!(rows.len(), waiting.len() + running.len());
        for task in waiting.iter().chain(running) {
            let row = rows.iter().find(|r| r.id == task["id"]).unwrap_or_else(|| panic!("{} lost", task["id"]));
            assert_eq!(row.repo, task["repo"]);
            assert_eq!(row.made_at, task["madeAt"].as_f64().unwrap());
            for key in ["name", "message", "by", "agent", "folder", "session"] {
                assert_eq!(row.doc.get(key).unwrap_or(&Value::Null), task.get(key).unwrap_or(&Value::Null), "{} {key}", task["id"]);
            }
            let made = &db.events(&row.id).unwrap()[0];
            assert_eq!((made.what.as_str(), made.at), ("made", row.made_at));
        }
        for task in running {
            let row = rows.iter().find(|r| r.id == task["id"]).unwrap();
            assert_eq!((row.place, row.pane.as_deref()), (Place::Shelf, task["pane"].as_str()));
        }
        assert!(rows.iter().filter(|r| r.place == Place::Backlog).all(|r| r.pane.is_none()));
        assert_eq!(db.counter().unwrap(), saved["counter"].as_u64().unwrap());
        assert_eq!(db.next_task().unwrap(), format!("t{}", saved["counter"].as_u64().unwrap() + 1));
        drop(db);
        let again = Db::open(&dir);
        assert_eq!(again.tasks().unwrap().len(), rows.len(), "nothing is imported twice");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_held_task_keeps_its_pane_and_a_bad_entry_is_skipped() {
        let dir = folder("held");
        let saved = json!({ "counter": 3, "tasks": [
            { "id": "t2", "name": "Held", "repo": "/r", "madeAt": 4.0, "agent": "claude", "held": "p9", "folder": "/r/w" },
            { "id": "p5", "repo": "/r" },
            { "id": "t3", "name": "No repo" },
        ]});
        fs::write(dir.join(BACKLOG), saved.to_string()).unwrap();
        let rows = Db::open(&dir).tasks().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0].place, rows[0].pane.as_deref()), (Place::Backlog, Some("p9")));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_change_and_its_event_are_written_together_and_unknown_doc_keys_are_kept() {
        let dir = folder("put");
        let db = Db::open(&dir);
        let id = db.next_task().unwrap();
        let mut row = Row { id: id.clone(), repo: "/r".into(), place: Place::Backlog, pane: None, made_at: 1.0, doc: json!({ "name": "A", "later": "kept" }) };
        db.put(&row, Some(&Change::new("made").by(Some("p1")))).unwrap();
        row.place = Place::Shelf;
        row.pane = Some("p7".into());
        row.doc = json!({ "name": "B", "session": null });
        db.put(&row, Some(&Change::new("shelved").moved(Place::Backlog, Place::Shelf).with("window", json!("w2")))).unwrap();
        let stored = db.tasks().unwrap().remove(0);
        assert_eq!(stored.doc, json!({ "name": "B", "later": "kept" }));
        assert_eq!((stored.place, stored.pane.as_deref()), (Place::Shelf, Some("p7")));
        let events = db.events(&id).unwrap();
        assert_eq!(events.iter().map(|e| e.what.as_str()).collect::<Vec<_>>(), ["made", "shelved"]);
        assert_eq!((events[1].from, events[1].to, events[1].doc["window"].as_str()), (Some(Place::Backlog), Some(Place::Shelf), Some("w2")));
        assert_eq!(events[0].by.as_deref(), Some("p1"));
        assert!(db.log("t404", &Change::new("made")).is_err(), "an event needs its task");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_corrupt_db_is_moved_aside_and_the_last_good_copy_restored() {
        let dir = folder("corrupt");
        fs::write(dir.join(BACKLOG), real().to_string()).unwrap();
        let count = Db::open(&dir).tasks().unwrap().len();
        assert!(dir.join(BACKUP).exists());
        let _ = fs::remove_file(dir.join(format!("{FILE}-wal")));
        let _ = fs::remove_file(dir.join(format!("{FILE}-shm")));
        fs::write(dir.join(FILE), b"not a database, just garbage that isn't sqlite at all ".repeat(100)).unwrap();
        let db = Db::open(&dir);
        assert_eq!(db.tasks().unwrap().len(), count, "the backup's tasks are back");
        assert!(fs::read_dir(&dir).unwrap().flatten().any(|f| f.file_name().to_string_lossy().starts_with("kanna.db.bad-")));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_missing_db_comes_back_from_its_copy_then_from_the_imported_backlog() {
        let dir = folder("missing");
        fs::write(dir.join(BACKLOG), real().to_string()).unwrap();
        let count = Db::open(&dir).tasks().unwrap().len();
        for file in [FILE.to_string(), format!("{FILE}-wal"), format!("{FILE}-shm")] {
            let _ = fs::remove_file(dir.join(file));
        }
        assert_eq!(Db::open(&dir).tasks().unwrap().len(), count, "from kanna.db.bak");
        for file in [FILE.to_string(), format!("{FILE}-wal"), format!("{FILE}-shm")] {
            let _ = fs::remove_file(dir.join(file));
        }
        fs::write(dir.join(BACKUP), b"garbage").unwrap();
        assert_eq!(Db::open(&dir).tasks().unwrap().len(), count, "from backlog.json.imported");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_db_from_a_newer_kanna_is_left_alone() {
        let dir = folder("newer");
        let conn = Connection::open(dir.join(FILE)).unwrap();
        conn.pragma_update(None, "user_version", version() + 1).unwrap();
        drop(conn);
        let db = Db::open(&dir);
        assert!(dir.join(format!("{FILE}.newer")).exists());
        assert!(db.tasks().unwrap().is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_backlog_json_written_beside_the_db_adds_only_the_tasks_it_lacks() {
        let dir = folder("beside");
        let db = Db::open(&dir);
        let id = db.next_task().unwrap();
        db.put(&Row { id: id.clone(), repo: "/r".into(), place: Place::Backlog, pane: None, made_at: 1.0, doc: json!({ "name": "In the db" }) }, Some(&Change::new("made"))).unwrap();
        drop(db);
        fs::write(dir.join(BACKLOG), json!({ "counter": 5, "tasks": [{ "id": id, "name": "Older", "repo": "/r" }, { "id": "t5", "name": "New", "repo": "/r", "madeAt": 2.0 }] }).to_string()).unwrap();
        let db = Db::open(&dir);
        let rows = db.tasks().unwrap();
        assert_eq!(rows.iter().map(|r| (r.id.as_str(), r.doc["name"].as_str().unwrap())).collect::<Vec<_>>(), [("t1", "In the db"), ("t5", "New")]);
        assert_eq!(db.next_task().unwrap(), "t6");
        fs::remove_dir_all(&dir).unwrap();
    }
}
