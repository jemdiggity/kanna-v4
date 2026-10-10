use kanna_detect::store::hex;
use kanna_detect::{Config, Screen, Status, Store};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

type Pages = Arc<Mutex<HashMap<String, (u16, String, String)>>>;

struct Site {
    base: String,
    pages: Pages,
    hits: Arc<Mutex<Vec<String>>>,
}

impl Site {
    fn start() -> Site {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let pages: Pages = Arc::default();
        let hits: Arc<Mutex<Vec<String>>> = Arc::default();
        let (served, seen) = (pages.clone(), hits.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let path = line.split_whitespace().nth(1).unwrap_or("/").to_string();
                loop {
                    let mut header = String::new();
                    if reader.read_line(&mut header).unwrap_or(0) == 0 || header == "\r\n" {
                        break;
                    }
                }
                seen.lock().unwrap().push(path.clone());
                let (status, extra, body) = served.lock().unwrap().get(&path).cloned().unwrap_or((404, String::new(), String::new()));
                let mut stream = stream;
                let _ = write!(stream, "HTTP/1.1 {status} X\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n{body}", body.len());
            }
        });
        Site { base, pages, hits }
    }

    fn page(&self, path: &str, body: &str) {
        self.pages.lock().unwrap().insert(path.into(), (200, String::new(), body.into()));
    }

    fn redirect(&self, path: &str, to: &str) {
        self.pages.lock().unwrap().insert(path.into(), (302, format!("Location: {to}\r\n"), String::new()));
    }

    fn index(&self) -> String {
        format!("{}/rules/index.json", self.base)
    }
}

fn rules(agent: &str, version: &str, marker: &str) -> String {
    format!(r#"{{"schema":1,"agent":"{agent}","version":"{version}","engine":1,"rules":[{{"id":"{agent}/needs-you/marker","status":"needs-you","priority":900,"when":{{"anyLine":{{"contains":"{marker}"}}}}}}]}}"#)
}

fn entry(agent: &str, url: &str, version: &str, body: &str) -> String {
    format!(r#"{{"agent":"{agent}","url":"{url}","version":"{version}","sha256":"{}"}}"#, hex(&Sha256::digest(body.as_bytes())))
}

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kanna-detect-store-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn store(name: &str, site: Option<&Site>) -> (Store, PathBuf) {
    let dir = folder(name);
    let mut config = Config::new(dir.join("cache"));
    config.overrides = Some(dir.join("overrides"));
    config.site = site.map(Site::index);
    config.watch_every = Duration::from_millis(50);
    (Store::new(config), dir)
}

fn judged(store: &Store, agent: &str, text: &str) -> (Status, String) {
    let v = store.rules().evaluate(agent, &Screen::new(text), None).unwrap();
    (v.status, v.source)
}

#[test]
fn a_newer_remote_file_is_hot_loaded() {
    let site = Site::start();
    let body = rules("codex", "2099.1.1.1", "REMOTE MARK");
    site.page("/rules/codex.json", &body);
    site.page("/rules/index.json", &format!(r#"{{"schema":1,"rules":[{}]}}"#, entry("codex", "codex.json", "2099.1.1.1", &body)));
    let (store, _) = store("newer", Some(&site));
    assert_eq!(judged(&store, "codex", "REMOTE MARK").0, Status::Idle);
    let report = store.update_now();
    assert_eq!(report.fetched, ["codex"], "{:?}", report.problems);
    let (status, source) = judged(&store, "codex", "REMOTE MARK");
    assert_eq!(status, Status::NeedsYou);
    assert!(source.starts_with("remote http://127.0.0.1"), "{source}");
}

#[test]
fn a_file_whose_hash_isnt_the_index_s_is_rejected() {
    let site = Site::start();
    let body = rules("codex", "2099.1.1.1", "REMOTE MARK");
    site.page("/rules/codex.json", &body.replace("REMOTE", "TAMPERED"));
    site.page("/rules/index.json", &format!(r#"{{"schema":1,"rules":[{}]}}"#, entry("codex", "codex.json", "2099.1.1.1", &body)));
    let (store, dir) = store("hash", Some(&site));
    let report = store.update_now();
    assert!(report.problems.iter().any(|p| p.contains("hash")), "{:?}", report.problems);
    assert_eq!(judged(&store, "codex", "TAMPERED MARK"), (Status::Idle, "bundled".into()));
    assert!(!dir.join("cache/remote/codex.json").exists());
}

#[test]
fn an_older_or_equal_remote_file_is_not_fetched() {
    let site = Site::start();
    let body = rules("codex", "2020.1.1.1", "OLD MARK");
    site.page("/rules/codex.json", &body);
    site.page("/rules/index.json", &format!(r#"{{"schema":1,"rules":[{}]}}"#, entry("codex", "codex.json", "2020.1.1.1", &body)));
    let (store, _) = store("older", Some(&site));
    assert!(store.update_now().fetched.is_empty());
    assert!(!site.hits.lock().unwrap().iter().any(|h| h.ends_with("codex.json")));
    assert_eq!(judged(&store, "codex", "OLD MARK").1, "bundled");
}

#[test]
fn files_on_another_host_are_rejected() {
    let site = Site::start();
    let body = rules("codex", "2099.1.1.1", "MARK");
    site.page("/rules/index.json", &format!(r#"{{"schema":1,"rules":[{}]}}"#, entry("codex", "https://example.com/codex.json", "2099.1.1.1", &body)));
    let (store, _) = store("host", Some(&site));
    let report = store.update_now();
    assert!(report.problems.iter().any(|p| p.contains("isn't on the rules site")), "{:?}", report.problems);
    assert_eq!(judged(&store, "codex", "MARK").1, "bundled");
}

#[test]
fn a_redirect_to_another_host_is_rejected() {
    let site = Site::start();
    let body = rules("codex", "2099.1.1.1", "MARK");
    site.redirect("/rules/codex.json", "http://localhost:1/codex.json");
    site.page("/rules/index.json", &format!(r#"{{"schema":1,"rules":[{}]}}"#, entry("codex", "codex.json", "2099.1.1.1", &body)));
    let (store, _) = store("redirect", Some(&site));
    let report = store.update_now();
    assert!(report.problems.iter().any(|p| p.contains("another host")), "{:?}", report.problems);
}

#[test]
fn index_entries_must_name_known_agents_safely() {
    let site = Site::start();
    let body = rules("codex", "2099.1.1.1", "MARK");
    site.page("/rules/codex.json", &body);
    let entries = ["../codex", "Codex", "stranger", "codex_x"].map(|name| entry(name, "codex.json", "2099.1.1.1", &body)).join(",");
    site.page("/rules/index.json", &format!(r#"{{"schema":1,"rules":[{entries}]}}"#));
    let (store, dir) = store("names", Some(&site));
    let report = store.update_now();
    assert_eq!(report.problems.iter().map(|p| p.matches("not an agent this Kanna knows").count()).sum::<usize>(), 4, "{:?}", report.problems);
    assert!(std::fs::read_dir(dir.join("cache/remote")).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().ends_with(".json")));
}

#[test]
fn a_file_that_isnt_the_index_s_agent_or_version_is_rejected() {
    let site = Site::start();
    let body = rules("claude", "2099.1.1.1", "MARK");
    site.page("/rules/codex.json", &body);
    site.page("/rules/index.json", &format!(r#"{{"schema":1,"rules":[{}]}}"#, entry("codex", "codex.json", "2099.1.1.1", &body)));
    let (store, _) = store("identity", Some(&site));
    let report = store.update_now();
    assert!(report.problems.iter().any(|p| p.contains("the index says")), "{:?}", report.problems);
}

#[test]
fn an_unsupported_index_is_rejected() {
    let site = Site::start();
    site.page("/rules/index.json", r#"{"schema":9,"rules":[]}"#);
    let (store, _) = store("schema", Some(&site));
    assert!(store.update_now().problems.iter().any(|p| p.contains("schema 9")));
}

#[test]
fn an_override_wins_reloads_when_it_changes_and_a_bad_one_keeps_the_last_good() {
    let (store, dir) = store("override", None);
    let _updater = store.start();
    let overrides = dir.join("overrides");
    std::fs::create_dir_all(&overrides).unwrap();
    std::fs::write(overrides.join("codex.json"), rules("codex", "1.0", "OVERRIDE ONE")).unwrap();
    let wait = |text: &str, want: Status| {
        for _ in 0..100 {
            if judged(&store, "codex", text).0 == want {
                return true;
            }
            std::thread::sleep(Duration::from_millis(30));
        }
        false
    };
    assert!(wait("OVERRIDE ONE", Status::NeedsYou), "{:?}", store.problems());
    assert!(judged(&store, "codex", "x").1.starts_with("override "));
    std::thread::sleep(Duration::from_millis(1100));
    std::fs::write(overrides.join("codex.json"), rules("codex", "1.0", "OVERRIDE TWO ")).unwrap();
    assert!(wait("OVERRIDE TWO ", Status::NeedsYou));
    std::thread::sleep(Duration::from_millis(1100));
    std::fs::write(overrides.join("codex.json"), "{ not json").unwrap();
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(judged(&store, "codex", "OVERRIDE TWO ").0, Status::NeedsYou);
    assert!(store.problems().iter().any(|p| p.contains("codex.json")));
    std::fs::write(overrides.join("stranger.json"), rules("codex", "1.0", "X")).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert!(store.rules().agent("stranger").is_none());
}
