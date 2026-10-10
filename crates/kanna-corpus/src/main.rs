use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use kanna_vt::{Lines, Mode, Terminal};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

const USAGE: &str = "usage: kanna-corpus RECORDING.jsonl --version VERSION --out DIR [--per-phase N]

Replays a tools/agent-harness recording through libghostty-vt, samples the
settled screen every quarter second, labels each sample by the phase the
harness marked (idle, working, needs-you) and writes the distinct ones to
DIR as corpus cases. Samples near a phase's edges are left out.";

const STEP: f64 = 0.25;
const SETTLE: f64 = 0.06;

struct Event {
    t: f64,
    kind: String,
    bytes: Vec<u8>,
    data: Value,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let option = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let (Some(recording), Some(version), Some(out)) = (args.first().filter(|a| !a.starts_with('-')), option("--version"), option("--out")) else {
        eprintln!("{USAGE}");
        std::process::exit(2);
    };
    let per_phase: usize = option("--per-phase").and_then(|n| n.parse().ok()).unwrap_or(6);
    match sample(Path::new(recording), &version, Path::new(&out), per_phase) {
        Ok(written) => println!("{recording}: {written} cases"),
        Err(error) => {
            eprintln!("kanna-corpus: {recording}: {error}");
            std::process::exit(1);
        }
    }
}

fn read(path: &Path) -> Result<Vec<Event>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let v: Value = serde_json::from_str(line).map_err(|e| e.to_string())?;
            let bytes = v.get("b").and_then(Value::as_str).map(|b| BASE64.decode(b).unwrap_or_default()).unwrap_or_default();
            Ok(Event { t: v["t"].as_f64().unwrap_or(0.0), kind: v["k"].as_str().unwrap_or("").into(), bytes, data: v.get("d").cloned().unwrap_or(Value::Null) })
        })
        .collect()
}

fn windows(events: &[Event]) -> Result<Vec<(&'static str, f64, f64)>, String> {
    let mark = |name: &str| events.iter().find(|e| e.kind == "mark" && e.data == name).map(|e| e.t).ok_or(format!("no {name} mark"));
    let enter_after = |t: f64| events.iter().find(|e| e.kind == "i" && e.t >= t && e.bytes == b"\r").map(|e| e.t).ok_or("no Enter".to_string());
    let matched_after = |t: f64| events.iter().find(|e| e.kind == "matched" && e.t >= t).map(|e| e.t).ok_or("no match".to_string());
    let (startup, plain, plain_end, tool, asking, approve, idle, end) =
        (mark("startup")?, mark("plain")?, mark("plain_end")?, mark("tool")?, mark("asking")?, mark("approve")?, mark("idle")?, mark("end")?);
    let ready = matched_after(startup)?;
    let (plain_enter, tool_enter) = (enter_after(plain)?, enter_after(tool)?);
    Ok(vec![
        ("idle", ready + 2.0, plain - 0.3),
        ("working", plain_enter + 0.6, plain_end - 0.3),
        ("idle", plain_end + 3.0, tool_enter - 0.3),
        ("working", tool_enter + 0.6, asking - 0.5),
        ("needs-you", asking + 0.5, approve - 0.2),
        ("working", approve + 0.6, idle - 0.5),
        ("idle", idle + 3.0, end - 0.5),
    ])
}

fn sample(recording: &Path, version: &str, out: &Path, per_phase: usize) -> Result<usize, String> {
    let events = read(recording)?;
    let start = events.iter().find(|e| e.kind == "start").ok_or("no start")?;
    let (cols, rows) = (start.data["cols"].as_u64().unwrap_or(120) as u16, start.data["rows"].as_u64().unwrap_or(40) as u16);
    let phases = windows(&events)?;
    let name = recording.file_stem().and_then(|s| s.to_str()).unwrap_or("recording");
    let mut terminal = Terminal::new(cols, rows, 1000).map_err(|e| e.to_string())?;
    let outputs: Vec<&Event> = events.iter().filter(|e| e.kind == "o").collect();
    let mut next = 0;
    let mut last_output = f64::NEG_INFINITY;
    let mut samples: Vec<(&'static str, usize, f64, Value)> = vec![];
    let end = events.last().map(|e| e.t).unwrap_or(0.0);
    let mut t = 0.0;
    while t <= end {
        while next < outputs.len() && outputs[next].t <= t {
            terminal.write(&outputs[next].bytes);
            last_output = outputs[next].t;
            next += 1;
        }
        let settled = t - last_output >= SETTLE && !terminal.mode(Mode::SYNC_OUTPUT);
        if let Some((phase, label, _, _)) = phases.iter().enumerate().find(|(_, (_, from, to))| t >= *from && t <= *to).map(|(i, (l, f, to))| (i, *l, f, to)) {
            if settled {
                let screen: Vec<String> = terminal.text(Lines::Screen).lines().map(str::to_owned).collect();
                let case = json!({
                    "rows": screen,
                    "title": terminal.title(),
                    "progress": terminal.progress().as_str(),
                    "cliVersion": version,
                    "expected": label,
                    "source": format!("tools/agent-harness {name} @ {t:.2}s"),
                    "size": format!("{cols}x{rows}"),
                });
                samples.push((label, phase, t, case));
            }
        }
        t = ((t / STEP).round() + 1.0) * STEP;
    }
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let mut written = 0;
    let mut seen = HashSet::new();
    for phase in 0..phases.len() {
        let distinct: Vec<&(&str, usize, f64, Value)> = samples
            .iter()
            .filter(|s| s.1 == phase)
            .filter(|s| seen.insert((s.3["rows"].to_string(), s.3["title"].to_string(), s.3["progress"].to_string())))
            .collect();
        let step = (distinct.len() as f64 / per_phase as f64).max(1.0);
        for k in 0..distinct.len().min(per_phase) {
            let (label, _, t, case) = distinct[(k as f64 * step) as usize];
            let file: PathBuf = out.join(format!("{name}-{label}-{t:06.2}.json"));
            std::fs::write(&file, serde_json::to_string_pretty(case).unwrap() + "\n").map_err(|e| e.to_string())?;
            written += 1;
        }
    }
    Ok(written)
}
