//! Runs the protocol cases shared with the reference model and the Swift
//! core (tests/kmux-protocol/cases, also run by tests/kmux-protocol/run-model.mjs
//! and ProtocolCasesTests.swift). Cases that use a command or pane type this
//! core doesn't have yet are skipped and listed, so the gap to the model
//! stays visible.

mod support;

use kanna_mux::json::{encode, num, Value};
use kanna_mux::{Core, DEBUG_COMMANDS};
use std::path::PathBuf;
use support::make_core;

fn cases_directory() -> PathBuf {
    // kmux is the reference model: its cases, from the kmux checkout beside
    // this repo (../kmux, or KMUX_DIR).
    let kmux = std::env::var_os("KMUX_DIR").map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../kmux"));
    kmux.join("tests/kmux-protocol/cases")
}

/// `expected` matches when every field it names matches; objects may have
/// extra fields, arrays must be the same length. Same rule as cases.mjs.
fn mismatch(expected: &Value, actual: Option<&Value>, path: &str) -> Option<String> {
    let actual = actual.unwrap_or(&Value::Null);
    match expected {
        Value::Array(items) => {
            let Value::Array(got) = actual else { return Some(format!("{path}: expected an array, got {}", encode(actual))) };
            if got.len() != items.len() {
                return Some(format!("{path}: expected {} items, got {}", items.len(), got.len()));
            }
            items.iter().zip(got).enumerate().find_map(|(i, (item, got))| mismatch(item, Some(got), &format!("{path}[{i}]")))
        }
        Value::Object(fields) => {
            let Value::Object(got) = actual else { return Some(format!("{path}: expected an object, got {}", encode(actual))) };
            fields.iter().find_map(|(key, value)| mismatch(value, got.get(key), &if path.is_empty() { key.clone() } else { format!("{path}.{key}") }))
        }
        _ => (!kanna_mux::json::same(expected, actual)).then(|| format!("{path}: expected {}, got {}", encode(expected), encode(actual))),
    }
}

fn pane_types(core: &Core) -> Vec<String> {
    let reply = core.handle_blocking(serde_json::json!({ "cmd": "capabilities" }));
    reply["paneTypes"].as_array().map(|t| t.iter().filter_map(|t| t.as_str().map(String::from)).collect()).unwrap_or_default()
}

#[test]
fn shared_protocol_cases() {
    let mut files: Vec<PathBuf> = std::fs::read_dir(cases_directory())
        .expect("no cases directory")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no cases in {}", cases_directory().display());
    let (probe, _) = make_core();
    let types = pane_types(&probe);
    let commands = probe.commands();

    let (mut passed, mut failed, mut skipped) = (0, vec![], vec![]);
    for file in &files {
        let name = file.file_name().unwrap().to_string_lossy();
        let cases: Value = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
        let Value::Array(cases) = cases else { panic!("{name} is not an array of cases") };
        for case in &cases {
            let title = format!("{name}: {}", case["name"].as_str().unwrap_or("?"));
            let Some(steps) = case["steps"].as_array() else { continue };
            let mut missing: Vec<String> = steps
                .iter()
                .filter_map(|step| {
                    let send = &step["send"];
                    let cmd = send["cmd"].as_str()?;
                    if !commands.iter().any(|c| c == cmd) && !DEBUG_COMMANDS.contains(&cmd) && cmd != "nope" {
                        return Some(cmd.to_string());
                    }
                    let kind = send["args"]["type"].as_str()?;
                    (cmd == "open" && ["term", "web", "ios"].contains(&kind) && !types.iter().any(|t| t == kind)).then(|| format!("{kind} panes"))
                })
                .collect();
            if !missing.is_empty() {
                missing.sort();
                missing.dedup();
                skipped.push(format!("{title} (needs {})", missing.join(", ")));
                continue;
            }
            let (core, _host) = make_core();
            let mut problem = None;
            for (index, step) in steps.iter().enumerate() {
                let Value::Object(mut request) = step["send"].clone() else { continue };
                request.insert("id".into(), num((index + 1) as f64));
                let reply = core.handle_blocking(Value::Object(request));
                let expected = step.get("expect").cloned().unwrap_or(serde_json::json!({ "ok": true }));
                if let Some(why) = mismatch(&expected, Some(&reply), "") {
                    problem = Some(format!("{title}\n  step {}: {why}\n  reply: {}", index + 1, encode(&reply)));
                    break;
                }
            }
            match problem {
                Some(problem) => failed.push(problem),
                None => passed += 1,
            }
        }
    }
    println!("native: {passed} cases passed, {} failed, {} skipped", failed.len(), skipped.len());
    for skip in &skipped {
        println!("  skipped {skip}");
    }
    assert!(failed.is_empty(), "{} cases failed:\n{}", failed.len(), failed.join("\n"));
}
