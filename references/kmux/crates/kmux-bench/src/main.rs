//! kmux-bench: the performance reference for kmux (docs/kmux-spec.md §8.3).
//!
//! Starts its own kmux in the background on a private socket (so it never
//! touches the user's kmux or covers their windows), then measures:
//!
//! - pane start: `open` until the pane is running, and until its first output
//!   is on screen (this includes the user's login shell starting);
//! - typing latency: a real key event until the character is in the
//!   terminal's text (`cat` echoes it through the tty), not counting the GPU
//!   frame that draws it;
//! - memory: the app's footprint per idle terminal pane.
//!
//! Results are printed, saved as JSON under target/bench/, and checked against
//! bench/targets.json. Exits 1 if a target is missed.

use kmux_client::{Kmux, Target};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::{Child, Command, ExitCode};
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const USAGE: &str = "usage: kmux-bench [--app PATH] [--starts N] [--keys N] [--panes N] [--json]

Measures pane start time, typing latency and memory per terminal pane on a
private kmux started in the background, and checks them against
bench/targets.json. --app defaults to target/kmux.app in this repo.";

struct Options {
    app: PathBuf,
    starts: usize,
    keys: usize,
    panes: usize,
    json: bool,
}

/// The kmux under test; stopped when dropped.
struct Mux {
    child: Child,
    socket: PathBuf,
    client: Kmux,
}

impl Drop for Mux {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.socket);
    }
}

impl Mux {
    fn start(app: &PathBuf) -> Result<Mux, String> {
        let socket = std::env::temp_dir().join(format!("kmux-bench-{}.sock", std::process::id()));
        let binary = app.join("Contents/MacOS/kmux");
        let child = Command::new(&binary)
            .arg("--bg")
            .env("KMUX_SOCKET", &socket)
            .env("KMUX_NO_INITIAL_WINDOW", "1")
            .env("KMUX_IGNORE_OCCLUSION", "1")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| format!("could not start {}: {e}", binary.display()))?;
        let target = Target { brand: kmux_client::Brand::KMUX, instance: "bench".into(), socket: socket.clone(), background: true };
        let deadline = Instant::now() + Duration::from_secs(10);
        let client = loop {
            if let Some(client) = Kmux::try_connect(&target) {
                break client;
            }
            if Instant::now() > deadline {
                return Err(format!("kmux did not open {}", socket.display()));
            }
            sleep(Duration::from_millis(50));
        };
        Ok(Mux { child, socket, client })
    }

    fn call(&mut self, cmd: &str, args: Value) -> Result<Value, String> {
        self.client.call(cmd, args).map_err(|e| format!("{cmd}: {e:?}"))
    }

    fn text(&mut self, pane: &str) -> Result<String, String> {
        Ok(self.call("debug.text", json!({ "pane": pane }))?["text"].as_str().unwrap_or_default().to_string())
    }

    /// Polls the pane's text until `done` says yes; returns when it did.
    fn wait_for(&mut self, pane: &str, timeout: Duration, done: impl Fn(&str) -> bool) -> Result<Instant, String> {
        let deadline = Instant::now() + timeout;
        loop {
            if done(&self.text(pane)?) {
                return Ok(Instant::now());
            }
            if Instant::now() > deadline {
                return Err(format!("timed out waiting for {pane}: {:?}", self.text(pane)?.trim_end()));
            }
        }
    }

    fn footprint(&mut self) -> Result<f64, String> {
        Ok(self.call("debug.stats", json!({}))?["footprint"].as_f64().unwrap_or(0.0))
    }
}

/// p50, p95 and max of some milliseconds.
fn summary(mut ms: Vec<f64>) -> Value {
    ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let at = |q: f64| ms[((ms.len() as f64 - 1.0) * q).round() as usize];
    json!({ "n": ms.len(), "p50": round(at(0.5)), "p95": round(at(0.95)), "max": round(at(1.0)) })
}

fn round(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

fn ms(since: Instant, at: Instant) -> f64 {
    at.duration_since(since).as_secs_f64() * 1000.0
}

/// Pane start: open a terminal whose command prints a marker, `starts` times.
fn bench_start(mux: &mut Mux, starts: usize) -> Result<Value, String> {
    let (mut running, mut output) = (vec![], vec![]);
    for i in 0..starts {
        let marker = format!("BENCH-START-{i}");
        let begin = Instant::now();
        let reply = mux.call("open", json!({ "type": "term", "tab": true, "cmd": format!("echo {marker}; exec cat") }))?;
        let ready = Instant::now();
        let pane = reply["pane"]["id"].as_str().unwrap_or_default().to_string();
        let shown = mux.wait_for(&pane, Duration::from_secs(10), |text| text.contains(&marker))?;
        running.push(ms(begin, ready));
        output.push(ms(begin, shown));
        mux.call("close", json!({ "pane": pane }))?;
    }
    Ok(json!({ "running": summary(running), "firstOutput": summary(output) }))
}

/// Typing: real key events into `cat`, timed until each character shows.
fn bench_typing(mux: &mut Mux, keys: usize) -> Result<Value, String> {
    let reply = mux.call("open", json!({ "type": "term", "tab": true, "cmd": "echo BENCH-TYPE; exec cat" }))?;
    let pane = reply["pane"]["id"].as_str().unwrap_or_default().to_string();
    mux.wait_for(&pane, Duration::from_secs(10), |text| text.contains("BENCH-TYPE"))?;
    sleep(Duration::from_millis(300));
    let mut typed = String::new();
    let mut samples = vec![];
    for i in 0..keys {
        if typed.len() == 30 {
            mux.call("debug.key", json!({ "key": "return" }))?;
            sleep(Duration::from_millis(50));
            typed.clear();
        }
        let key = (b'a' + (i % 26) as u8) as char;
        typed.push(key);
        let expected = typed.clone();
        let begin = Instant::now();
        mux.call("debug.key", json!({ "key": key.to_string() }))?;
        let shown = mux.wait_for(&pane, Duration::from_secs(2), |text| text.trim_end().lines().last() == Some(expected.as_str()))?;
        samples.push(ms(begin, shown));
    }
    mux.call("close", json!({ "pane": pane }))?;
    Ok(summary(samples))
}

/// Memory: the app's footprint before and after `panes` idle shells.
fn bench_memory(mux: &mut Mux, panes: usize) -> Result<Value, String> {
    let first = mux.call("open", json!({ "type": "term", "window": "new" }))?;
    sleep(Duration::from_secs(1));
    let before = mux.footprint()?;
    let mut opened = vec![];
    for _ in 0..panes {
        opened.push(mux.call("open", json!({ "type": "term", "tab": true }))?["pane"]["id"].as_str().unwrap_or_default().to_string());
    }
    sleep(Duration::from_secs(2));
    let after = mux.footprint()?;
    for pane in opened.iter().chain([first["pane"]["id"].as_str().unwrap_or_default().to_string()].iter()) {
        mux.call("close", json!({ "pane": pane }))?;
    }
    let mb = |bytes: f64| round(bytes / 1_048_576.0);
    Ok(json!({ "panes": panes, "withOnePaneMB": mb(before), "perTermPaneMB": mb((after - before) / panes as f64) }))
}

/// Compares results with targets: [(name, value, target, ok)].
fn check(results: &Value, targets: &Value) -> Vec<(String, f64, f64, bool)> {
    let rows = [
        ("pane start: first output p95 (ms)", &results["start"]["firstOutput"]["p95"], &targets["startFirstOutputP95Ms"]),
        ("typing p95 (ms)", &results["typing"]["p95"], &targets["typingP95Ms"]),
        ("memory per terminal pane (MB)", &results["memory"]["perTermPaneMB"], &targets["memoryPerTermPaneMB"]),
    ];
    rows.iter()
        .filter_map(|(name, value, target)| {
            let (value, target) = (value.as_f64()?, target.as_f64()?);
            Some((name.to_string(), value, target, value <= target))
        })
        .collect()
}

fn parse(args: Vec<String>, root: &PathBuf) -> Result<Options, String> {
    let mut options = Options { app: root.join("target/kmux.app"), starts: 10, keys: 100, panes: 10, json: false };
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let mut number = |name: &str| -> Result<usize, String> {
            args.next().and_then(|v| v.parse().ok()).filter(|n| *n > 0).ok_or(format!("{name} needs a positive number"))
        };
        match arg.as_str() {
            "--app" => options.app = PathBuf::from(args.next().ok_or("--app needs a PATH")?),
            "--starts" => options.starts = number("--starts")?,
            "--keys" => options.keys = number("--keys")?,
            "--panes" => options.panes = number("--panes")?,
            "--json" => options.json = true,
            "-h" | "--help" => return Err(String::new()),
            other => return Err(format!("unknown argument \"{other}\"")),
        }
    }
    Ok(options)
}

fn main() -> ExitCode {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root = root.canonicalize().unwrap_or(root);
    let options = match parse(std::env::args().skip(1).collect(), &root) {
        Ok(options) => options,
        Err(message) if message.is_empty() => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(message) => {
            eprintln!("kmux-bench: {message}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match run(&options, &root) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(message) => {
            eprintln!("kmux-bench: {message}");
            ExitCode::from(3)
        }
    }
}

fn run(options: &Options, root: &PathBuf) -> Result<bool, String> {
    let mut mux = Mux::start(&options.app)?;
    // A window to work in; benchmarks add and close tabs in it.
    mux.call("open", json!({ "type": "term", "window": "new" }))?;
    let start = bench_start(&mut mux, options.starts)?;
    let typing = bench_typing(&mut mux, options.keys)?;
    let memory = bench_memory(&mut mux, options.panes)?;
    drop(mux);

    let when = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let commit = Command::new("git").args(["-C", &root.to_string_lossy(), "rev-parse", "--short", "HEAD"]).output()
        .ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    let results = json!({ "time": when, "commit": commit, "app": options.app, "start": start, "typing": typing, "memory": memory });
    let targets: Value = std::fs::read_to_string(root.join("bench/targets.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(json!({}));
    let checks = check(&results, &targets);

    let dir = root.join("target/bench");
    let _ = std::fs::create_dir_all(&dir);
    let text = serde_json::to_string_pretty(&results).unwrap();
    let _ = std::fs::write(dir.join(format!("{when}.json")), &text);
    let _ = std::fs::write(dir.join("latest.json"), &text);

    if options.json {
        println!("{text}");
    } else {
        let s = |v: &Value| format!("p50 {:>6.1} ms   p95 {:>6.1} ms   max {:>6.1} ms   (n={})", v["p50"].as_f64().unwrap_or(0.0), v["p95"].as_f64().unwrap_or(0.0), v["max"].as_f64().unwrap_or(0.0), v["n"]);
        println!("kmux-bench @ {commit}");
        println!("  pane start, open → running        {}", s(&start["running"]));
        println!("  pane start, open → first output   {}", s(&start["firstOutput"]));
        println!("  typing, key → character on screen {}", s(&typing));
        println!("  memory: app with one pane {} MB, each more terminal pane {} MB ({} panes)", memory["withOnePaneMB"], memory["perTermPaneMB"], memory["panes"]);
        println!();
        if checks.is_empty() {
            println!("No targets set (bench/targets.json).");
        }
        for (name, value, target, ok) in &checks {
            println!("  {} {name}: {value} (target ≤ {target})", if *ok { "✓" } else { "✗" });
        }
        println!("\nSaved: {}", dir.join("latest.json").display());
    }
    Ok(checks.iter().all(|c| c.3))
}
